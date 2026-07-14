//! Immutable resolved and fully typed frontend IR.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::source::{OriginId, SourceContext};

/// Scalar value types accepted by the first source-language tranche.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ValueType {
    /// A boolean value.
    Bool,
    /// A signed 32-bit integer value.
    Int32,
}

impl fmt::Display for ValueType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool => formatter.write_str("Bool"),
            Self::Int32 => formatter.write_str("Int32"),
        }
    }
}

/// A source function's result contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FunctionResult {
    /// The function produces no value.
    Void,
    /// The function produces one scalar value.
    Value(ValueType),
}

impl fmt::Display for FunctionResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Void => formatter.write_str("Void"),
            Self::Value(ty) => write!(formatter, "{ty}"),
        }
    }
}

/// Dense identity of one source function in declaration order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceFunctionId(u32);

impl SourceFunctionId {
    /// Returns this identity's zero-based declaration index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }

    pub(super) fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    pub(super) fn as_usize(self) -> Option<usize> {
        usize::try_from(self.0).ok()
    }
}

/// Dense identity of a parameter or local within one owning function.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct LocalId(u32);

impl LocalId {
    pub(super) fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    pub(super) const fn index(self) -> u32 {
        self.0
    }

    fn as_usize(self) -> Option<usize> {
        usize::try_from(self.0).ok()
    }
}

/// Read-only successful product of name resolution, type checking, and flow checking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedFrontendOutput {
    module: HirModule,
}

impl CheckedFrontendOutput {
    pub(super) const fn new(module: HirModule) -> Self {
        Self { module }
    }

    /// Returns the number of source functions in declaration order.
    #[must_use]
    pub fn function_count(&self) -> usize {
        self.module.functions.len()
    }

    /// Returns whether the checked module contains no functions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.module.functions.is_empty()
    }

    /// Iterates source function IDs in deterministic declaration order.
    #[must_use]
    pub fn function_ids(&self) -> impl ExactSizeIterator<Item = SourceFunctionId> + '_ {
        self.module.functions.iter().map(|function| function.id)
    }

    /// Returns a function's result contract, or `None` for an ID outside this output.
    #[must_use]
    pub fn function_result(&self, function: SourceFunctionId) -> Option<FunctionResult> {
        self.function(function).map(|function| function.result)
    }

    /// Returns a function's parameter count, or `None` for an ID outside this output.
    #[must_use]
    pub fn parameter_count(&self, function: SourceFunctionId) -> Option<usize> {
        self.function(function)
            .map(|function| function.parameter_count)
    }

    /// Returns one positional parameter type, or `None` for an invalid function or index.
    #[must_use]
    pub fn parameter_type(
        &self,
        function: SourceFunctionId,
        parameter: usize,
    ) -> Option<ValueType> {
        let function = self.function(function)?;
        (parameter < function.parameter_count)
            .then(|| function.bindings.get(parameter).map(|binding| binding.ty))
            .flatten()
    }

    pub(super) fn function_name_origin(&self, function: SourceFunctionId) -> Option<OriginId> {
        self.function(function).map(|function| function.name_origin)
    }

    /// Produces a deterministic semantic dump using source spellings from `sources`.
    ///
    /// Missing or foreign provenance is rendered explicitly instead of panicking.
    #[must_use]
    pub fn dump(&self, sources: &SourceContext) -> String {
        Dumper::new(self, sources).dump()
    }

    pub(super) fn functions(&self) -> &[HirFunction] {
        &self.module.functions
    }

    fn function(&self, id: SourceFunctionId) -> Option<&HirFunction> {
        self.module.functions.get(id.as_usize()?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirModule {
    pub(super) functions: Box<[HirFunction]>,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirFunction {
    pub(super) id: SourceFunctionId,
    pub(super) name_origin: OriginId,
    pub(super) parameter_count: usize,
    pub(super) result: FunctionResult,
    pub(super) result_origin: Option<OriginId>,
    pub(super) bindings: Box<[HirBinding]>,
    pub(super) body: HirBlock,
    pub(super) origin: OriginId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HirBindingKind {
    Parameter,
    Const,
    Var,
}

impl HirBindingKind {
    pub(super) const fn is_mutable(self) -> bool {
        matches!(self, Self::Var)
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Parameter => "parameter",
            Self::Const => "const",
            Self::Var => "var",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirBinding {
    pub(super) id: LocalId,
    pub(super) kind: HirBindingKind,
    pub(super) ty: ValueType,
    pub(super) name_origin: OriginId,
    pub(super) type_origin: OriginId,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirBlock {
    pub(super) statements: Box<[HirStatement]>,
    pub(super) closing_brace_origin: OriginId,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirStatement {
    pub(super) kind: HirStatementKind,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirStatementKind {
    Declaration {
        local: LocalId,
        initializer: Option<HirExpression>,
    },
    Assignment {
        target: LocalId,
        value: HirExpression,
    },
    Call(HirCall),
    If(HirIf),
    Return(Option<HirExpression>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirIf {
    pub(super) arms: Box<[HirIfArm]>,
    pub(super) else_body: Option<HirBlock>,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirIfArm {
    pub(super) condition: HirExpression,
    pub(super) body: HirBlock,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirCall {
    pub(super) callee: SourceFunctionId,
    pub(super) arguments: Box<[HirExpression]>,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirExpression {
    pub(super) kind: HirExpressionKind,
    pub(super) ty: ValueType,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirExpressionKind {
    Bool(bool),
    Int32(i32),
    Local(LocalId),
    Call(HirCall),
    Not(Box<HirExpression>),
    Compare {
        op: HirComparisonOp,
        left: Box<HirExpression>,
        right: Box<HirExpression>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HirComparisonOp {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

impl HirComparisonOp {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
        }
    }

    pub(super) const fn is_ordered(self) -> bool {
        matches!(
            self,
            Self::Less | Self::LessEqual | Self::Greater | Self::GreaterEqual
        )
    }
}

struct Dumper<'a> {
    output: &'a CheckedFrontendOutput,
    sources: &'a SourceContext,
    text: String,
}

impl<'a> Dumper<'a> {
    const fn new(output: &'a CheckedFrontendOutput, sources: &'a SourceContext) -> Self {
        Self {
            output,
            sources,
            text: String::new(),
        }
    }

    fn dump(mut self) -> String {
        self.line(
            0,
            &format!("module {}", self.location(self.output.module.origin)),
        );
        for function in self.output.functions() {
            self.dump_function(function);
        }
        self.text
    }

    fn dump_function(&mut self, function: &HirFunction) {
        let mut parameters = Vec::with_capacity(function.parameter_count);
        for binding in function.bindings.iter().take(function.parameter_count) {
            parameters.push(format!(
                "%{} {}: {}",
                binding.id.index(),
                self.spelling(binding.name_origin),
                binding.ty
            ));
        }
        self.line(
            1,
            &format!(
                "fn @{} {}({}) -> {} {}",
                function.id.index(),
                self.spelling(function.name_origin),
                parameters.join(", "),
                function.result,
                self.location(function.origin)
            ),
        );
        self.line(2, "bindings");
        for binding in &function.bindings {
            self.line(
                3,
                &format!(
                    "%{} {} {}: {} {}",
                    binding.id.index(),
                    binding.kind.as_str(),
                    self.spelling(binding.name_origin),
                    binding.ty,
                    self.location(binding.origin)
                ),
            );
        }
        self.dump_block(&function.body, 2);
    }

    fn dump_block(&mut self, block: &HirBlock, indent: usize) {
        self.line(indent, &format!("block {}", self.location(block.origin)));
        for statement in &block.statements {
            self.dump_statement(statement, indent + 1);
        }
    }

    fn dump_statement(&mut self, statement: &HirStatement, indent: usize) {
        match &statement.kind {
            HirStatementKind::Declaration { local, initializer } => {
                let suffix = initializer.as_ref().map_or_else(String::new, |expression| {
                    format!(" = {}", self.expression(expression))
                });
                self.line(
                    indent,
                    &format!(
                        "declare %{}{} {}",
                        local.index(),
                        suffix,
                        self.location(statement.origin)
                    ),
                );
            }
            HirStatementKind::Assignment { target, value } => self.line(
                indent,
                &format!(
                    "assign %{} = {} {}",
                    target.index(),
                    self.expression(value),
                    self.location(statement.origin)
                ),
            ),
            HirStatementKind::Call(call) => self.line(
                indent,
                &format!(
                    "discard {} {}",
                    self.call(call),
                    self.location(statement.origin)
                ),
            ),
            HirStatementKind::If(conditional) => {
                self.line(indent, &format!("if {}", self.location(conditional.origin)));
                for arm in &conditional.arms {
                    self.line(
                        indent + 1,
                        &format!(
                            "when {} {}",
                            self.expression(&arm.condition),
                            self.location(arm.origin)
                        ),
                    );
                    self.dump_block(&arm.body, indent + 2);
                }
                if let Some(else_body) = &conditional.else_body {
                    self.line(indent + 1, "else");
                    self.dump_block(else_body, indent + 2);
                }
            }
            HirStatementKind::Return(value) => {
                let value = value.as_ref().map_or_else(
                    || "return".to_owned(),
                    |value| format!("return {}", self.expression(value)),
                );
                self.line(
                    indent,
                    &format!("{} {}", value, self.location(statement.origin)),
                );
            }
        }
    }

    fn expression(&self, expression: &HirExpression) -> String {
        let value = match &expression.kind {
            HirExpressionKind::Bool(value) => value.to_string(),
            HirExpressionKind::Int32(value) => value.to_string(),
            HirExpressionKind::Local(local) => format!("%{}", local.index()),
            HirExpressionKind::Call(call) => self.call(call),
            HirExpressionKind::Not(operand) => format!("!({})", self.expression(operand)),
            HirExpressionKind::Compare { op, left, right } => format!(
                "({} {} {})",
                self.expression(left),
                op.as_str(),
                self.expression(right)
            ),
        };
        format!("{value}:{}", expression.ty)
    }

    fn call(&self, call: &HirCall) -> String {
        let arguments = call
            .arguments
            .iter()
            .map(|argument| self.expression(argument))
            .collect::<Vec<_>>()
            .join(", ");
        format!("call @{}({arguments})", call.callee.index())
    }

    fn spelling(&self, origin: OriginId) -> String {
        self.sources
            .resolve_origin_span(origin)
            .and_then(|span| self.sources.files().slice(span).ok())
            .unwrap_or("<unknown>")
            .to_owned()
    }

    fn location(&self, origin: OriginId) -> String {
        self.sources.resolve_origin_span(origin).map_or_else(
            || "@?".to_owned(),
            |span| format!("@{}..{}", span.start(), span.end()),
        )
    }

    fn line(&mut self, indent: usize, value: &str) {
        self.text.push_str(&"  ".repeat(indent));
        self.text.push_str(value);
        self.text.push('\n');
    }
}

/// Internal HIR invariant failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirVerificationError {
    message: String,
}

impl HirVerificationError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for HirVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for HirVerificationError {}

pub(super) fn verify(
    output: &CheckedFrontendOutput,
    sources: &SourceContext,
) -> Result<(), HirVerificationError> {
    Verifier { output, sources }.verify()
}

struct Verifier<'a> {
    output: &'a CheckedFrontendOutput,
    sources: &'a SourceContext,
}

impl Verifier<'_> {
    fn verify(&self) -> Result<(), HirVerificationError> {
        self.origin(self.output.module.origin, "module")?;
        for (index, function) in self.output.functions().iter().enumerate() {
            let expected = SourceFunctionId::from_index(index)
                .ok_or_else(|| HirVerificationError::new("function identity space exhausted"))?;
            if function.id != expected {
                return Err(HirVerificationError::new(format!(
                    "function at index {index} has non-dense identity {:?}",
                    function.id
                )));
            }
            self.verify_function(function)?;
        }
        Ok(())
    }

    fn verify_function(&self, function: &HirFunction) -> Result<(), HirVerificationError> {
        self.origin(function.origin, "function")?;
        self.origin(function.name_origin, "function name")?;
        if function.parameter_count > function.bindings.len() {
            return Err(HirVerificationError::new(format!(
                "function {:?} has more parameters than bindings",
                function.id
            )));
        }
        match (function.result, function.result_origin) {
            (FunctionResult::Value(_), None) => {
                return Err(HirVerificationError::new(format!(
                    "value function {:?} has no result origin",
                    function.id
                )));
            }
            (_, Some(origin)) => self.origin(origin, "function result")?,
            (FunctionResult::Void, None) => {}
        }

        for (index, binding) in function.bindings.iter().enumerate() {
            let expected = LocalId::from_index(index)
                .ok_or_else(|| HirVerificationError::new("local identity space exhausted"))?;
            if binding.id != expected {
                return Err(HirVerificationError::new(format!(
                    "function {:?} binding at index {index} has non-dense identity {:?}",
                    function.id, binding.id
                )));
            }
            let should_be_parameter = index < function.parameter_count;
            if (binding.kind == HirBindingKind::Parameter) != should_be_parameter {
                return Err(HirVerificationError::new(format!(
                    "function {:?} binding {:?} has an invalid parameter classification",
                    function.id, binding.id
                )));
            }
            self.origin(binding.origin, "binding")?;
            self.origin(binding.name_origin, "binding name")?;
            self.origin(binding.type_origin, "binding type")?;
        }

        let mut state = VerifyState::new(function.bindings.len());
        for index in 0..function.parameter_count {
            state.set_active(index, true);
            state.set_assigned(index, true);
        }
        let mut next_declaration = function.parameter_count;
        let continues =
            self.verify_block(function, &function.body, &mut state, &mut next_declaration)?;
        if matches!(function.result, FunctionResult::Value(_)) && continues {
            return Err(HirVerificationError::new(format!(
                "value function {:?} has a continuing exit",
                function.id
            )));
        }
        if next_declaration != function.bindings.len() {
            return Err(HirVerificationError::new(format!(
                "function {:?} declares {next_declaration} of {} binding inventory entries",
                function.id,
                function.bindings.len()
            )));
        }
        Ok(())
    }

    fn verify_block(
        &self,
        function: &HirFunction,
        block: &HirBlock,
        state: &mut VerifyState,
        next_declaration: &mut usize,
    ) -> Result<bool, HirVerificationError> {
        self.origin(block.origin, "block")?;
        self.origin(block.closing_brace_origin, "block closing brace")?;
        let mut scope_locals = vec![];
        let mut continues = true;
        for statement in &block.statements {
            let statement_continues = self.verify_statement(
                function,
                statement,
                state,
                next_declaration,
                &mut scope_locals,
            )?;
            if continues && !statement_continues {
                continues = false;
            }
        }
        for local in scope_locals {
            let index = Self::local_index(function, local)?;
            state.set_active(index, false);
            state.set_assigned(index, false);
        }
        Ok(continues)
    }

    fn verify_statement(
        &self,
        function: &HirFunction,
        statement: &HirStatement,
        state: &mut VerifyState,
        next_declaration: &mut usize,
        scope_locals: &mut Vec<LocalId>,
    ) -> Result<bool, HirVerificationError> {
        self.origin(statement.origin, "statement")?;
        match &statement.kind {
            HirStatementKind::Declaration { local, initializer } => {
                let index = Self::local_index(function, *local)?;
                let binding = &function.bindings[index];
                if binding.kind == HirBindingKind::Parameter {
                    return Err(HirVerificationError::new(format!(
                        "binding {local:?} is declared as a parameter"
                    )));
                }
                if index != *next_declaration {
                    return Err(HirVerificationError::new(format!(
                        "binding {local:?} is out of source order; expected binding index {}",
                        *next_declaration
                    )));
                }
                if let Some(initializer) = initializer {
                    self.expression(function, initializer, state)?;
                    if initializer.ty != binding.ty {
                        return Err(HirVerificationError::new(format!(
                            "binding {local:?} initializer has type {}, expected {}",
                            initializer.ty, binding.ty
                        )));
                    }
                } else if binding.kind == HirBindingKind::Const {
                    return Err(HirVerificationError::new(format!(
                        "const binding {local:?} has no initializer"
                    )));
                }
                *next_declaration += 1;
                state.set_active(index, true);
                state.set_assigned(index, initializer.is_some());
                scope_locals.push(*local);
                Ok(true)
            }
            HirStatementKind::Assignment { target, value } => {
                let index = Self::active_local_index(function, *target, state)?;
                let binding = &function.bindings[index];
                if !binding.kind.is_mutable() {
                    return Err(HirVerificationError::new(format!(
                        "assignment targets immutable binding {target:?}"
                    )));
                }
                self.expression(function, value, state)?;
                if value.ty != binding.ty {
                    return Err(HirVerificationError::new(format!(
                        "assignment to {target:?} has type {}, expected {}",
                        value.ty, binding.ty
                    )));
                }
                state.set_assigned(index, true);
                Ok(true)
            }
            HirStatementKind::Call(call) => {
                self.call(function, call, state)?;
                Ok(true)
            }
            HirStatementKind::If(conditional) => {
                self.verify_if(function, conditional, state, next_declaration)
            }
            HirStatementKind::Return(value) => {
                match (function.result, value) {
                    (FunctionResult::Void, None) => {}
                    (FunctionResult::Value(expected), Some(value)) => {
                        self.expression(function, value, state)?;
                        if value.ty != expected {
                            return Err(HirVerificationError::new(format!(
                                "return has type {}, expected {expected}",
                                value.ty
                            )));
                        }
                    }
                    (FunctionResult::Void, Some(_)) => {
                        return Err(HirVerificationError::new("Void function returns a value"));
                    }
                    (FunctionResult::Value(_), None) => {
                        return Err(HirVerificationError::new(
                            "value function has an empty return",
                        ));
                    }
                }
                Ok(false)
            }
        }
    }

    fn verify_if(
        &self,
        function: &HirFunction,
        conditional: &HirIf,
        state: &mut VerifyState,
        next_declaration: &mut usize,
    ) -> Result<bool, HirVerificationError> {
        self.origin(conditional.origin, "conditional")?;
        if conditional.arms.is_empty() {
            return Err(HirVerificationError::new("conditional has no arms"));
        }
        let checkpoint = state.checkpoint();
        let mut continuing_delta = None;
        for arm in &conditional.arms {
            self.origin(arm.origin, "conditional arm")?;
            self.expression(function, &arm.condition, state)?;
            if arm.condition.ty != ValueType::Bool {
                return Err(HirVerificationError::new(
                    "conditional arm condition is not Bool",
                ));
            }
            if self.verify_block(function, &arm.body, state, next_declaration)? {
                let delta = state.newly_assigned_since(checkpoint);
                VerifyState::merge_delta_intersection(&mut continuing_delta, &delta);
            }
            state.rollback(checkpoint);
        }
        if let Some(else_body) = &conditional.else_body {
            if self.verify_block(function, else_body, state, next_declaration)? {
                let delta = state.newly_assigned_since(checkpoint);
                VerifyState::merge_delta_intersection(&mut continuing_delta, &delta);
            }
            state.rollback(checkpoint);
        } else {
            VerifyState::merge_delta_intersection(&mut continuing_delta, &[]);
        }
        if let Some(continuing_delta) = continuing_delta {
            for index in continuing_delta {
                state.set_assigned(index, true);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expression(
        &self,
        function: &HirFunction,
        expression: &HirExpression,
        state: &VerifyState,
    ) -> Result<(), HirVerificationError> {
        self.origin(expression.origin, "expression")?;
        let inferred = match &expression.kind {
            HirExpressionKind::Bool(_) => ValueType::Bool,
            HirExpressionKind::Int32(_) => ValueType::Int32,
            HirExpressionKind::Local(local) => {
                let index = Self::active_local_index(function, *local, state)?;
                if !state.assigned[index] {
                    return Err(HirVerificationError::new(format!(
                        "expression reads unassigned binding {local:?}"
                    )));
                }
                function.bindings[index].ty
            }
            HirExpressionKind::Call(call) => {
                let result = self.call(function, call, state)?;
                let FunctionResult::Value(ty) = result else {
                    return Err(HirVerificationError::new(
                        "Void call appears in value expression",
                    ));
                };
                ty
            }
            HirExpressionKind::Not(operand) => {
                self.expression(function, operand, state)?;
                if operand.ty != ValueType::Bool {
                    return Err(HirVerificationError::new("`!` operand is not Bool"));
                }
                ValueType::Bool
            }
            HirExpressionKind::Compare { op, left, right } => {
                self.expression(function, left, state)?;
                self.expression(function, right, state)?;
                if op.is_ordered() {
                    if left.ty != ValueType::Int32 || right.ty != ValueType::Int32 {
                        return Err(HirVerificationError::new(
                            "ordered comparison operands are not Int32",
                        ));
                    }
                } else if left.ty != right.ty {
                    return Err(HirVerificationError::new(
                        "equality comparison operand types differ",
                    ));
                }
                ValueType::Bool
            }
        };
        if expression.ty != inferred {
            return Err(HirVerificationError::new(format!(
                "expression records type {}, but its kind produces {inferred}",
                expression.ty
            )));
        }
        Ok(())
    }

    fn call(
        &self,
        owner: &HirFunction,
        call: &HirCall,
        state: &VerifyState,
    ) -> Result<FunctionResult, HirVerificationError> {
        self.origin(call.origin, "call")?;
        let callee = self.output.function(call.callee).ok_or_else(|| {
            HirVerificationError::new(format!("invalid callee {:?}", call.callee))
        })?;
        if call.arguments.len() != callee.parameter_count {
            return Err(HirVerificationError::new(format!(
                "call to {:?} has {} arguments, expected {}",
                call.callee,
                call.arguments.len(),
                callee.parameter_count
            )));
        }
        for (index, argument) in call.arguments.iter().enumerate() {
            self.expression(owner, argument, state)?;
            let expected = callee.bindings.get(index).ok_or_else(|| {
                HirVerificationError::new(format!(
                    "callee {:?} is missing parameter binding {index}",
                    call.callee
                ))
            })?;
            if argument.ty != expected.ty {
                return Err(HirVerificationError::new(format!(
                    "call argument {index} has type {}, expected {}",
                    argument.ty, expected.ty
                )));
            }
        }
        Ok(callee.result)
    }

    fn local_index(function: &HirFunction, local: LocalId) -> Result<usize, HirVerificationError> {
        let index = local.as_usize().ok_or_else(|| {
            HirVerificationError::new(format!("invalid local identity {local:?}"))
        })?;
        if index >= function.bindings.len() {
            return Err(HirVerificationError::new(format!(
                "local {local:?} is outside function {:?}",
                function.id
            )));
        }
        Ok(index)
    }

    fn active_local_index(
        function: &HirFunction,
        local: LocalId,
        state: &VerifyState,
    ) -> Result<usize, HirVerificationError> {
        let index = Self::local_index(function, local)?;
        if !state.active[index] {
            return Err(HirVerificationError::new(format!(
                "local {local:?} is referenced outside its active scope"
            )));
        }
        Ok(index)
    }

    fn origin(&self, origin: OriginId, role: &'static str) -> Result<(), HirVerificationError> {
        if self.sources.origin(origin).is_none()
            || self.sources.resolve_origin_span(origin).is_none()
        {
            return Err(HirVerificationError::new(format!(
                "{role} has invalid or unresolvable provenance {origin:?}"
            )));
        }
        Ok(())
    }
}

struct VerifyState {
    active: Vec<bool>,
    assigned: Vec<bool>,
    changes: Vec<VerifyStateChange>,
}

#[derive(Clone, Copy)]
enum VerifyStateChange {
    Active { index: usize, previous: bool },
    Assigned { index: usize, previous: bool },
}

impl VerifyState {
    fn new(binding_count: usize) -> Self {
        Self {
            active: vec![false; binding_count],
            assigned: vec![false; binding_count],
            changes: vec![],
        }
    }

    fn set_active(&mut self, index: usize, value: bool) {
        if self.active[index] == value {
            return;
        }
        self.changes.push(VerifyStateChange::Active {
            index,
            previous: self.active[index],
        });
        self.active[index] = value;
    }

    fn set_assigned(&mut self, index: usize, value: bool) {
        if self.assigned[index] == value {
            return;
        }
        self.changes.push(VerifyStateChange::Assigned {
            index,
            previous: self.assigned[index],
        });
        self.assigned[index] = value;
    }

    fn checkpoint(&self) -> usize {
        self.changes.len()
    }

    fn rollback(&mut self, checkpoint: usize) {
        while self.changes.len() > checkpoint {
            match self
                .changes
                .pop()
                .expect("the change log is longer than the checkpoint")
            {
                VerifyStateChange::Active { index, previous } => self.active[index] = previous,
                VerifyStateChange::Assigned { index, previous } => {
                    self.assigned[index] = previous;
                }
            }
        }
    }

    fn newly_assigned_since(&self, checkpoint: usize) -> Vec<usize> {
        let mut initial = BTreeMap::new();
        for change in &self.changes[checkpoint..] {
            if let VerifyStateChange::Assigned { index, previous } = *change {
                initial.entry(index).or_insert(previous);
            }
        }
        initial
            .into_iter()
            .filter_map(|(index, previous)| {
                (!previous && self.assigned.get(index) == Some(&true)).then_some(index)
            })
            .collect()
    }

    fn merge_delta_intersection(intersection: &mut Option<Vec<usize>>, delta: &[usize]) {
        let Some(current) = intersection else {
            *intersection = Some(delta.to_vec());
            return;
        };
        current.retain(|index| delta.binary_search(index).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HirBindingKind, HirExpressionKind, HirStatementKind, LocalId, SourceFunctionId, verify,
    };
    use crate::frontend::FrontendLimits;
    use crate::frontend::check::{CheckOutput, check};
    use crate::frontend::lexer::lex;
    use crate::frontend::parser::parse;
    use crate::source::{OriginId, SourceContext};

    fn checked_text(text: &str) -> (SourceContext, CheckOutput) {
        let mut sources = SourceContext::new();
        let file = sources.add_file("test.mdl", text).unwrap();
        let lexed = lex(&mut sources, file, FrontendLimits::DEFAULT).unwrap();
        assert_eq!(lexed.diagnostics(), None);
        let parsed = parse(&mut sources, file, lexed.tokens(), FrontendLimits::DEFAULT).unwrap();
        assert_eq!(parsed.diagnostics(), None);
        let (module, _) = parsed.into_parts();
        let output = check(&mut sources, &module, FrontendLimits::DEFAULT).unwrap();
        assert_eq!(output.diagnostics(), None);
        (sources, output)
    }

    #[test]
    fn verifier_rejects_non_dense_function_identity() {
        let (sources, output) = checked_text("fn valid() {}");
        let (checked, _) = output.into_parts();
        let mut checked = checked.unwrap();
        checked.module.functions[0].id = SourceFunctionId(7);

        let error = verify(&checked, &sources).unwrap_err();
        assert!(error.to_string().contains("non-dense identity"));
    }

    #[test]
    fn verifier_rejects_foreign_local_references() {
        let (sources, output) =
            checked_text("fn identity(value: Int32) -> Int32 { return value; }");
        let (checked, _) = output.into_parts();
        let mut checked = checked.unwrap();
        let HirStatementKind::Return(Some(expression)) =
            &mut checked.module.functions[0].body.statements[0].kind
        else {
            unreachable!();
        };
        expression.kind = HirExpressionKind::Local(LocalId(99));

        let error = verify(&checked, &sources).unwrap_err();
        assert!(error.to_string().contains("outside function"));
    }

    #[test]
    fn verifier_rejects_malformed_forward_callee_without_panicking() {
        let (sources, output) =
            checked_text("fn caller() { callee(1); } fn callee(value: Int32) {}");
        let (checked, _) = output.into_parts();
        let mut checked = checked.unwrap();
        checked.module.functions[1].bindings = Box::new([]);

        let error = verify(&checked, &sources).unwrap_err();
        assert!(error.to_string().contains("missing parameter binding 0"));
    }

    #[test]
    fn verifier_rejects_declarations_outside_dense_source_order() {
        let (sources, output) =
            checked_text("fn valid() { var first: Int32 = 0; var second: Int32 = 1; }");
        let (checked, _) = output.into_parts();
        let mut checked = checked.unwrap();
        checked.module.functions[0].body.statements.swap(0, 1);

        let error = verify(&checked, &sources).unwrap_err();
        assert!(error.to_string().contains("out of source order"));
    }

    #[test]
    fn verifier_rechecks_assignment_mutability_and_expression_types() {
        let (sources, output) = checked_text("fn valid() { var value: Int32 = 0; value = 1; }");
        let (checked, _) = output.into_parts();
        let mut immutable = checked.unwrap();
        immutable.module.functions[0].bindings[0].kind = HirBindingKind::Const;
        let error = verify(&immutable, &sources).unwrap_err();
        assert!(error.to_string().contains("immutable binding"));

        let (sources, output) = checked_text("fn valid() -> Int32 { return 1; }");
        let (checked, _) = output.into_parts();
        let mut mistyped = checked.unwrap();
        let HirStatementKind::Return(Some(expression)) =
            &mut mistyped.module.functions[0].body.statements[0].kind
        else {
            unreachable!();
        };
        expression.ty = super::ValueType::Bool;
        let error = verify(&mistyped, &sources).unwrap_err();
        assert!(error.to_string().contains("its kind produces Int32"));
    }

    #[test]
    fn verifier_rejects_unresolvable_provenance() {
        let (sources, output) = checked_text("fn valid() {}");
        let (checked, _) = output.into_parts();
        let mut checked = checked.unwrap();
        checked.module.functions[0].origin = OriginId::UNKNOWN;

        let error = verify(&checked, &sources).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid or unresolvable provenance")
        );
    }
}
