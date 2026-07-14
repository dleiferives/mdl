//! Syntax-shaped representation produced by the Stage 6 parser.

#[cfg(test)]
use std::fmt;
#[cfg(test)]
use std::fmt::Write as _;

#[cfg(test)]
use crate::source::SourceContext;
use crate::source::Span;

/// One parsed single-file compilation unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstModule {
    pub(super) functions: Vec<AstFunction>,
    pub(super) span: Span,
}

/// One source spelling used as a name before resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AstName {
    pub(super) span: Span,
}

/// Source value types accepted by the first scalar grammar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AstValueTypeKind {
    Bool,
    Int32,
}

/// One explicitly written source value type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AstValueType {
    pub(super) kind: AstValueTypeKind,
    pub(super) span: Span,
}

/// One explicitly written function result contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AstResultTypeKind {
    Value(AstValueTypeKind),
    Void,
}

/// Result contract introduced by a source `->`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AstResultType {
    pub(super) kind: AstResultTypeKind,
    pub(super) span: Span,
}

/// One parsed positional function parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstParameter {
    pub(super) name: AstName,
    pub(super) ty: AstValueType,
    pub(super) span: Span,
}

/// One parsed function definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstFunction {
    pub(super) name: AstName,
    pub(super) parameters: Vec<AstParameter>,
    /// Omission means the same result contract as explicit `Void`.
    pub(super) result: Option<AstResultType>,
    pub(super) body: AstBlock,
    pub(super) span: Span,
}

/// One lexical source block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstBlock {
    pub(super) statements: Vec<AstStatement>,
    pub(super) closing_brace: Span,
    pub(super) span: Span,
}

/// Whether a source declaration is immutable or writable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AstBindingKind {
    Const,
    Var,
}

/// One parsed local declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstDeclaration {
    pub(super) kind: AstBindingKind,
    pub(super) name: AstName,
    pub(super) ty: AstValueType,
    pub(super) initializer: Option<AstExpression>,
    pub(super) span: Span,
}

/// One parsed assignment to a source name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstAssignment {
    pub(super) target: AstName,
    pub(super) value: AstExpression,
    pub(super) span: Span,
}

/// One condition/body pair in an `if` / `else if` chain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstIfArm {
    pub(super) condition: AstExpression,
    pub(super) body: AstBlock,
    pub(super) span: Span,
}

/// One complete parsed conditional chain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstIfStatement {
    pub(super) arms: Vec<AstIfArm>,
    pub(super) else_body: Option<AstBlock>,
    pub(super) span: Span,
}

/// One parsed explicit return.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstReturnStatement {
    pub(super) value: Option<AstExpression>,
    pub(super) span: Span,
}

/// Statements accepted by the first scalar grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstStatement {
    Declaration(AstDeclaration),
    Assignment(AstAssignment),
    Call(AstCallStatement),
    If(AstIfStatement),
    Return(AstReturnStatement),
    /// A required statement that parser recovery could not construct.
    Error(Span),
}

/// One direct source call before name resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstCall {
    pub(super) callee: AstName,
    pub(super) arguments: Vec<AstExpression>,
    pub(super) span: Span,
}

/// One call used as a semicolon-terminated discard statement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstCallStatement {
    pub(super) call: AstCall,
    pub(super) span: Span,
}

/// Closed comparison vocabulary accepted by the first scalar grammar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AstComparisonOp {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

/// One parsed expression with its complete source range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstExpression {
    pub(super) kind: AstExpressionKind,
    pub(super) span: Span,
}

/// Expression forms accepted by the first scalar grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstExpressionKind {
    Bool(bool),
    /// The original decimal spelling is recovered from this span during checking.
    DecimalInteger(Span),
    Name(AstName),
    Call(AstCall),
    Not(Box<AstExpression>),
    Compare {
        op: AstComparisonOp,
        left: Box<AstExpression>,
        right: Box<AstExpression>,
    },
    /// A required expression that parser recovery could not construct.
    Error,
}

/// Dumps parsed structure deterministically for frontend tests and debugging.
#[cfg(test)]
pub(super) fn dump(module: &AstModule, sources: &SourceContext) -> String {
    let mut printer = AstPrinter {
        sources,
        output: String::new(),
    };
    printer.line(0, format_args!("module {}", location(module.span)));
    for function in &module.functions {
        printer.function(function);
    }
    printer.output
}

#[cfg(test)]
struct AstPrinter<'a> {
    sources: &'a SourceContext,
    output: String,
}

#[cfg(test)]
impl AstPrinter<'_> {
    fn function(&mut self, function: &AstFunction) {
        self.line(
            1,
            format_args!(
                "function {} {}",
                self.spelling(function.name.span),
                location(function.span)
            ),
        );
        for parameter in &function.parameters {
            self.line(
                2,
                format_args!(
                    "parameter {}: {} {}",
                    self.spelling(parameter.name.span),
                    value_type(parameter.ty.kind),
                    location(parameter.span)
                ),
            );
        }
        match function.result {
            Some(result) => self.line(
                2,
                format_args!(
                    "result {} {}",
                    result_type(result.kind),
                    location(result.span)
                ),
            ),
            None => self.line(2, format_args!("result Void (omitted)")),
        }
        self.block(&function.body, 2);
    }

    fn block(&mut self, block: &AstBlock, indent: usize) {
        self.line(indent, format_args!("block {}", location(block.span)));
        for statement in &block.statements {
            self.statement(statement, indent + 1);
        }
    }

    fn statement(&mut self, statement: &AstStatement, indent: usize) {
        match statement {
            AstStatement::Declaration(declaration) => {
                let kind = match declaration.kind {
                    AstBindingKind::Const => "const",
                    AstBindingKind::Var => "var",
                };
                self.line(
                    indent,
                    format_args!(
                        "{kind} {}: {} {}",
                        self.spelling(declaration.name.span),
                        value_type(declaration.ty.kind),
                        location(declaration.span)
                    ),
                );
                if let Some(initializer) = &declaration.initializer {
                    self.expression(initializer, indent + 1);
                }
            }
            AstStatement::Assignment(assignment) => {
                self.line(
                    indent,
                    format_args!(
                        "assign {} {}",
                        self.spelling(assignment.target.span),
                        location(assignment.span)
                    ),
                );
                self.expression(&assignment.value, indent + 1);
            }
            AstStatement::Call(statement) => {
                self.line(
                    indent,
                    format_args!("discard-call {}", location(statement.span)),
                );
                self.call(&statement.call, indent + 1);
            }
            AstStatement::If(conditional) => {
                self.line(indent, format_args!("if {}", location(conditional.span)));
                for arm in &conditional.arms {
                    self.line(indent + 1, format_args!("arm {}", location(arm.span)));
                    self.expression(&arm.condition, indent + 2);
                    self.block(&arm.body, indent + 2);
                }
                if let Some(body) = &conditional.else_body {
                    self.line(indent + 1, format_args!("else"));
                    self.block(body, indent + 2);
                }
            }
            AstStatement::Return(return_statement) => {
                self.line(
                    indent,
                    format_args!("return {}", location(return_statement.span)),
                );
                if let Some(value) = &return_statement.value {
                    self.expression(value, indent + 1);
                }
            }
            AstStatement::Error(span) => {
                self.line(indent, format_args!("error {}", location(*span)));
            }
        }
    }

    fn expression(&mut self, expression: &AstExpression, indent: usize) {
        match &expression.kind {
            AstExpressionKind::Bool(value) => self.line(
                indent,
                format_args!("bool {value} {}", location(expression.span)),
            ),
            AstExpressionKind::DecimalInteger(span) => self.line(
                indent,
                format_args!(
                    "integer {} {}",
                    self.spelling(*span),
                    location(expression.span)
                ),
            ),
            AstExpressionKind::Name(name) => self.line(
                indent,
                format_args!(
                    "name {} {}",
                    self.spelling(name.span),
                    location(expression.span)
                ),
            ),
            AstExpressionKind::Call(call) => {
                self.line(indent, format_args!("call {}", location(expression.span)));
                self.call(call, indent + 1);
            }
            AstExpressionKind::Not(operand) => {
                self.line(indent, format_args!("not {}", location(expression.span)));
                self.expression(operand, indent + 1);
            }
            AstExpressionKind::Compare { op, left, right } => {
                self.line(
                    indent,
                    format_args!("compare {} {}", comparison(*op), location(expression.span)),
                );
                self.expression(left, indent + 1);
                self.expression(right, indent + 1);
            }
            AstExpressionKind::Error => {
                self.line(
                    indent,
                    format_args!("error-expression {}", location(expression.span)),
                );
            }
        }
    }

    fn call(&mut self, call: &AstCall, indent: usize) {
        self.line(
            indent,
            format_args!("callee {}", self.spelling(call.callee.span)),
        );
        for argument in &call.arguments {
            self.expression(argument, indent + 1);
        }
    }

    fn spelling(&self, span: Span) -> String {
        self.sources
            .files()
            .slice(span)
            .unwrap_or("<invalid-span>")
            .to_owned()
    }

    fn line(&mut self, indent: usize, arguments: fmt::Arguments<'_>) {
        for _ in 0..indent {
            self.output.push_str("  ");
        }
        self.output
            .write_fmt(arguments)
            .expect("formatting into String cannot fail");
        self.output.push('\n');
    }
}

#[cfg(test)]
const fn value_type(ty: AstValueTypeKind) -> &'static str {
    match ty {
        AstValueTypeKind::Bool => "Bool",
        AstValueTypeKind::Int32 => "Int32",
    }
}

#[cfg(test)]
const fn result_type(ty: AstResultTypeKind) -> &'static str {
    match ty {
        AstResultTypeKind::Value(value) => value_type(value),
        AstResultTypeKind::Void => "Void",
    }
}

#[cfg(test)]
const fn comparison(op: AstComparisonOp) -> &'static str {
    match op {
        AstComparisonOp::Equal => "eq",
        AstComparisonOp::NotEqual => "ne",
        AstComparisonOp::Less => "lt",
        AstComparisonOp::LessEqual => "le",
        AstComparisonOp::Greater => "gt",
        AstComparisonOp::GreaterEqual => "ge",
    }
}

#[cfg(test)]
fn location(span: Span) -> String {
    format!("@{}..{}", span.start(), span.end())
}
