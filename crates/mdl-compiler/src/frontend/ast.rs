//! Syntax-shaped representation produced by the source parser.

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
    pub(super) imports: Vec<AstImport>,
    pub(super) structs: Vec<AstStruct>,
    pub(super) enums: Vec<AstEnum>,
    pub(super) functions: Vec<AstFunction>,
    pub(super) span: Span,
}

/// One closed fieldless nominal enum declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstEnum {
    pub(super) name: AstName,
    pub(super) variants: Vec<AstEnumVariant>,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstEnumVariant {
    pub(super) name: AstName,
    pub(super) span: Span,
}

/// One Zig-style module namespace binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstImport {
    pub(super) binding: AstName,
    /// Complete quoted and lexically validated dependency-name literal.
    pub(super) dependency: Span,
    pub(super) span: Span,
}

/// One nominal fixed-layout type declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstStruct {
    pub(super) name: AstName,
    pub(super) fields: Vec<AstStructField>,
    pub(super) span: Span,
}

/// One source-ordered field in a nominal struct.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstStructField {
    pub(super) name: AstName,
    pub(super) ty: AstValueType,
    pub(super) span: Span,
}

/// One source spelling used as a name before resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AstName {
    pub(super) span: Span,
}

/// Source value types accepted by the first scalar grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstValueTypeKind {
    Bool,
    Int32,
    ListI32,
    String,
    Named(AstName),
    Anonymous(AstAnonymousStructType),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstAnonymousStructType {
    pub(super) kind: AstAnonymousStructTypeKind,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstAnonymousStructTypeKind {
    Named(Vec<AstStructField>),
    Positional(Vec<AstValueType>),
}

/// One explicitly written source value type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstValueType {
    pub(super) kind: AstValueTypeKind,
    pub(super) span: Span,
}

/// One explicitly written function result contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstResultTypeKind {
    Value(AstValueTypeKind),
    Void,
}

/// Result contract introduced by a source `->`.
#[derive(Clone, Debug, Eq, PartialEq)]
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
    pub(super) visibility: AstFunctionVisibility,
    pub(super) visibility_span: Option<Span>,
    pub(super) name: AstName,
    pub(super) parameters: Vec<AstParameter>,
    /// Omission means the same result contract as explicit `Void`.
    pub(super) result: Option<AstResultType>,
    pub(super) body: AstBlock,
    pub(super) span: Span,
}

/// Source visibility and datapack-entry intent of one function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AstFunctionVisibility {
    Private,
    Public,
    Export,
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
    pub(super) ty: Option<AstValueType>,
    pub(super) initializer: Option<AstExpression>,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstDestructuringStatement {
    pub(super) targets: Vec<AstDestructureTarget>,
    pub(super) value: AstExpression,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstDestructureTarget {
    pub(super) kind: AstDestructureTargetKind,
    pub(super) name: AstName,
    pub(super) span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AstDestructureTargetKind {
    Const,
    Var,
    Assign,
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

/// One structured pre-test loop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstWhileStatement {
    pub(super) condition: AstExpression,
    pub(super) body: AstBlock,
    pub(super) span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AstSignedInteger {
    pub(super) negative: bool,
    pub(super) digits: Span,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstSwitchPatternKind {
    Integer(AstSignedInteger),
    IntegerRange {
        min: AstSignedInteger,
        max: AstSignedInteger,
    },
    EnumVariant {
        ty: Option<AstName>,
        variant: AstName,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstSwitchPattern {
    pub(super) kind: AstSwitchPatternKind,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstSwitchLabel {
    Patterns(Vec<AstSwitchPattern>),
    Else(Span),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstSwitchStatementArm {
    pub(super) label: AstSwitchLabel,
    pub(super) body: AstBlock,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstSwitchStatement {
    pub(super) scrutinee: AstExpression,
    pub(super) arms: Vec<AstSwitchStatementArm>,
    pub(super) span: Span,
}

/// One parsed explicit return.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstReturnStatement {
    pub(super) value: Option<AstExpression>,
    pub(super) span: Span,
}

/// One literal-only unsafe Minecraft command statement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstUnsafeMinecraftStatement {
    /// Complete quoted and lexically validated command literal.
    pub(super) command: Span,
    pub(super) span: Span,
}

/// One ordered modifier in a contextual `run` statement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstRunModifier {
    pub(super) name: AstName,
    pub(super) arguments: Vec<AstExpression>,
    pub(super) span: Span,
}

/// One contextual block with ordered modifiers and an optional executor capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstRunStatement {
    pub(super) modifiers: Vec<AstRunModifier>,
    pub(super) capture: Option<AstName>,
    pub(super) body: AstBlock,
    pub(super) span: Span,
}

/// Statements accepted by the first scalar grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstStatement {
    Declaration(AstDeclaration),
    Assignment(AstAssignment),
    Destructure(AstDestructuringStatement),
    Call(AstCallStatement),
    If(AstIfStatement),
    While(AstWhileStatement),
    Switch(AstSwitchStatement),
    Break(Span),
    Continue(Span),
    Return(AstReturnStatement),
    Run(AstRunStatement),
    UnsafeMinecraft(AstUnsafeMinecraftStatement),
    /// A required statement that parser recovery could not construct.
    Error(Span),
}

/// One postfix source call before name or member resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstCall {
    pub(super) callee: Box<AstExpression>,
    pub(super) arguments: Vec<AstExpression>,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstStructLiteral {
    pub(super) ty: AstName,
    pub(super) fields: Vec<AstStructFieldInitializer>,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstStructFieldInitializer {
    pub(super) name: AstName,
    pub(super) value: AstExpression,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstInferredStructLiteral {
    pub(super) entries: AstInferredStructEntries,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstInferredStructEntries {
    Named(Vec<AstStructFieldInitializer>),
    Positional(Vec<AstExpression>),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AstWrappingArithmeticOp {
    Add,
    Subtract,
}

/// One parsed expression with its complete source range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstExpression {
    pub(super) kind: AstExpressionKind,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstSwitchExpressionArm {
    pub(super) label: AstSwitchLabel,
    pub(super) body: AstExpression,
    pub(super) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AstSwitchExpression {
    pub(super) scrutinee: Box<AstExpression>,
    pub(super) arms: Vec<AstSwitchExpressionArm>,
    pub(super) span: Span,
}

/// Expression forms accepted by the first scalar grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AstExpressionKind {
    Bool(bool),
    /// The original decimal spelling is recovered from this span during checking.
    DecimalInteger(Span),
    /// A compiler-known decimal coordinate atom, optionally prefixed by `~` or `^`.
    StaticDecimal {
        sigil: Option<AstCoordinateSigil>,
        negative: bool,
        digits: Option<Span>,
    },
    /// Complete quoted and lexically validated literal spelling.
    StringLiteral(Span),
    InferredEnumLiteral(AstName),
    Switch(AstSwitchExpression),
    Name(AstName),
    Member {
        receiver: Box<AstExpression>,
        dot: Span,
        member: AstName,
    },
    Call(AstCall),
    StructLiteral(AstStructLiteral),
    InferredStructLiteral(AstInferredStructLiteral),
    Index {
        aggregate: Box<AstExpression>,
        index: Span,
    },
    Not(Box<AstExpression>),
    WrappingArithmetic {
        op: AstWrappingArithmeticOp,
        left: Box<AstExpression>,
        right: Box<AstExpression>,
    },
    Compare {
        op: AstComparisonOp,
        left: Box<AstExpression>,
        right: Box<AstExpression>,
    },
    /// A required expression that parser recovery could not construct.
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AstCoordinateSigil {
    Relative,
    Local,
}

/// Dumps parsed structure deterministically for frontend tests and debugging.
#[cfg(test)]
pub(super) fn dump(module: &AstModule, sources: &SourceContext) -> String {
    let mut printer = AstPrinter {
        sources,
        output: String::new(),
    };
    printer.line(0, format_args!("module {}", location(module.span)));
    for import in &module.imports {
        printer.line(
            1,
            format_args!(
                "import {} = {} {}",
                printer.spelling(import.binding.span),
                printer.spelling(import.dependency),
                location(import.span)
            ),
        );
    }
    for struct_ in &module.structs {
        printer.struct_(struct_);
    }
    for enum_ in &module.enums {
        printer.line(
            1,
            format_args!(
                "enum {} {}",
                printer.spelling(enum_.name.span),
                location(enum_.span)
            ),
        );
        for variant in &enum_.variants {
            printer.line(
                2,
                format_args!(
                    "variant {} {}",
                    printer.spelling(variant.name.span),
                    location(variant.span)
                ),
            );
        }
    }
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
    fn struct_(&mut self, struct_: &AstStruct) {
        self.line(
            1,
            format_args!(
                "struct {} {}",
                self.spelling(struct_.name.span),
                location(struct_.span)
            ),
        );
        for field in &struct_.fields {
            self.line(
                2,
                format_args!(
                    "field {}: {} {}",
                    self.spelling(field.name.span),
                    self.value_type(field.ty.kind),
                    location(field.span)
                ),
            );
        }
    }

    fn function(&mut self, function: &AstFunction) {
        let visibility = match function.visibility {
            AstFunctionVisibility::Private => "private",
            AstFunctionVisibility::Public => "public",
            AstFunctionVisibility::Export => "export",
        };
        self.line(
            1,
            format_args!(
                "{visibility} function {} {}",
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
                    self.value_type(parameter.ty.kind),
                    location(parameter.span)
                ),
            );
        }
        match function.result {
            Some(result) => self.line(
                2,
                format_args!(
                    "result {} {}",
                    self.result_type(result.kind),
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

    #[allow(
        clippy::too_many_lines,
        reason = "the test-only AST dump keeps its closed statement vocabulary exhaustive"
    )]
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
                        self.value_type(declaration.ty.kind),
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
            AstStatement::While(statement) => {
                self.line(indent, format_args!("while {}", location(statement.span)));
                self.line(indent + 1, format_args!("condition"));
                self.expression(&statement.condition, indent + 2);
                self.block(&statement.body, indent + 1);
            }
            AstStatement::Switch(switch) => {
                self.line(indent, format_args!("switch {}", location(switch.span)));
                self.expression(&switch.scrutinee, indent + 1);
                for arm in &switch.arms {
                    self.switch_label(&arm.label, indent + 1);
                    self.block(&arm.body, indent + 2);
                }
            }
            AstStatement::Break(span) => {
                self.line(indent, format_args!("break {}", location(*span)));
            }
            AstStatement::Continue(span) => {
                self.line(indent, format_args!("continue {}", location(*span)));
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
            AstStatement::Run(run) => {
                self.line(indent, format_args!("run {}", location(run.span)));
                for modifier in &run.modifiers {
                    self.line(
                        indent + 1,
                        format_args!(
                            "modifier {} {}",
                            self.spelling(modifier.name.span),
                            location(modifier.span)
                        ),
                    );
                    for argument in &modifier.arguments {
                        self.expression(argument, indent + 2);
                    }
                }
                if let Some(capture) = run.capture {
                    self.line(
                        indent + 1,
                        format_args!(
                            "capture {} {}",
                            self.spelling(capture.span),
                            location(capture.span)
                        ),
                    );
                }
                self.block(&run.body, indent + 1);
            }
            AstStatement::Destructure(destructure) => {
                self.line(
                    indent,
                    format_args!("destructure {}", location(destructure.span)),
                );
                for target in &destructure.targets {
                    let role = match target.kind {
                        AstDestructureTargetKind::Const => "const",
                        AstDestructureTargetKind::Var => "var",
                        AstDestructureTargetKind::Assign => "assign",
                    };
                    self.line(
                        indent + 1,
                        format_args!(
                            "target {role} {} {}",
                            self.spelling(target.name.span),
                            location(target.span)
                        ),
                    );
                }
                self.expression(&destructure.value, indent + 1);
            }
            AstStatement::UnsafeMinecraft(statement) => self.line(
                indent,
                format_args!(
                    "unsafe minecraft {} {}",
                    self.spelling(statement.command),
                    location(statement.span)
                ),
            ),
            AstStatement::Error(span) => {
                self.line(indent, format_args!("error {}", location(*span)));
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the test-only AST dump keeps its closed expression vocabulary exhaustive"
    )]
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
            AstExpressionKind::StaticDecimal {
                sigil,
                negative,
                digits,
            } => self.line(
                indent,
                format_args!(
                    "spatial {:?} negative={} digits={} {}",
                    sigil,
                    negative,
                    digits.map_or_else(|| "<zero>".to_owned(), |span| self.spelling(span)),
                    location(expression.span)
                ),
            ),
            AstExpressionKind::StringLiteral(span) => self.line(
                indent,
                format_args!(
                    "string {} {}",
                    self.spelling(*span),
                    location(expression.span)
                ),
            ),
            AstExpressionKind::InferredEnumLiteral(name) => self.line(
                indent,
                format_args!(
                    "enum-literal .{} {}",
                    self.spelling(name.span),
                    location(expression.span)
                ),
            ),
            AstExpressionKind::Switch(switch) => {
                self.line(
                    indent,
                    format_args!("switch-expression {}", location(switch.span)),
                );
                self.expression(&switch.scrutinee, indent + 1);
                for arm in &switch.arms {
                    self.switch_label(&arm.label, indent + 1);
                    self.expression(&arm.body, indent + 2);
                }
            }
            AstExpressionKind::Name(name) => self.line(
                indent,
                format_args!(
                    "name {} {}",
                    self.spelling(name.span),
                    location(expression.span)
                ),
            ),
            AstExpressionKind::Member {
                receiver,
                dot,
                member,
            } => {
                self.line(
                    indent,
                    format_args!(
                        "member {} dot={} {}",
                        self.spelling(member.span),
                        location(*dot),
                        location(expression.span)
                    ),
                );
                self.expression(receiver, indent + 1);
            }
            AstExpressionKind::Call(call) => {
                self.line(indent, format_args!("call {}", location(expression.span)));
                self.call(call, indent + 1);
            }
            AstExpressionKind::StructLiteral(literal) => {
                self.line(
                    indent,
                    format_args!(
                        "struct-literal {} {}",
                        self.spelling(literal.ty.span),
                        location(expression.span)
                    ),
                );
                for field in &literal.fields {
                    self.line(
                        indent + 1,
                        format_args!(
                            "field {} {}",
                            self.spelling(field.name.span),
                            location(field.span)
                        ),
                    );
                    self.expression(&field.value, indent + 2);
                }
            }
            AstExpressionKind::Not(operand) => {
                self.line(indent, format_args!("not {}", location(expression.span)));
                self.expression(operand, indent + 1);
            }
            AstExpressionKind::WrappingArithmetic { op, left, right } => {
                let operator = match op {
                    AstWrappingArithmeticOp::Add => "+%",
                    AstWrappingArithmeticOp::Subtract => "-%",
                };
                self.line(
                    indent,
                    format_args!(
                        "wrapping-arithmetic {operator} {}",
                        location(expression.span)
                    ),
                );
                self.expression(left, indent + 1);
                self.expression(right, indent + 1);
            }
            AstExpressionKind::Compare { op, left, right } => {
                self.line(
                    indent,
                    format_args!("compare {} {}", comparison(*op), location(expression.span)),
                );
                self.expression(left, indent + 1);
                self.expression(right, indent + 1);
            }
            AstExpressionKind::Index { aggregate, .. } => {
                self.line(indent, format_args!("index {}", location(expression.span)));
                self.expression(aggregate, indent + 1);
            }
            AstExpressionKind::InferredStructLiteral(literal) => {
                self.line(
                    indent,
                    format_args!("inferred-struct-literal {}", location(expression.span)),
                );
                match &literal.entries {
                    AstInferredStructEntries::Named(fields) => {
                        for field in fields {
                            self.line(
                                indent + 1,
                                format_args!(
                                    "field {} {}",
                                    self.spelling(field.name.span),
                                    location(field.span)
                                ),
                            );
                            self.expression(&field.value, indent + 2);
                        }
                    }
                    AstInferredStructEntries::Positional(values) => {
                        for value in values {
                            self.expression(value, indent + 1);
                        }
                    }
                }
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
        self.line(indent, format_args!("callee"));
        self.expression(&call.callee, indent + 1);
        for argument in &call.arguments {
            self.expression(argument, indent + 1);
        }
    }

    fn switch_label(&mut self, label: &AstSwitchLabel, indent: usize) {
        match label {
            AstSwitchLabel::Else(span) => {
                self.line(indent, format_args!("else {}", location(*span)));
            }
            AstSwitchLabel::Patterns(patterns) => {
                self.line(indent, format_args!("patterns"));
                for pattern in patterns {
                    let text = match &pattern.kind {
                        AstSwitchPatternKind::Integer(value) => self.spelling(value.span),
                        AstSwitchPatternKind::IntegerRange { min, max } => {
                            format!("{}...{}", self.spelling(min.span), self.spelling(max.span))
                        }
                        AstSwitchPatternKind::EnumVariant { ty, variant } => ty.map_or_else(
                            || format!(".{}", self.spelling(variant.span)),
                            |ty| {
                                format!(
                                    "{}.{}",
                                    self.spelling(ty.span),
                                    self.spelling(variant.span)
                                )
                            },
                        ),
                    };
                    self.line(
                        indent + 1,
                        format_args!("{text} {}", location(pattern.span)),
                    );
                }
            }
        }
    }

    fn spelling(&self, span: Span) -> String {
        self.sources
            .files()
            .slice(span)
            .unwrap_or("<invalid-span>")
            .to_owned()
    }

    fn value_type(&self, ty: AstValueTypeKind) -> String {
        match ty {
            AstValueTypeKind::Bool => "Bool".to_owned(),
            AstValueTypeKind::Int32 => "Int32".to_owned(),
            AstValueTypeKind::ListI32 => "List<Int32>".to_owned(),
            AstValueTypeKind::String => "String".to_owned(),
            AstValueTypeKind::Named(name) => self.spelling(name.span),
            AstValueTypeKind::Anonymous(anon) => match anon.kind {
                AstAnonymousStructTypeKind::Named(fields) => {
                    let fields = fields
                        .iter()
                        .map(|field| {
                            format!(
                                "{}: {}",
                                self.spelling(field.name.span),
                                self.value_type(field.ty.kind)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{ {fields} }}")
                }
                AstAnonymousStructTypeKind::Positional(types) => {
                    let types = types
                        .iter()
                        .map(|ty| self.value_type(ty.kind))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{ {types} }}")
                }
            },
        }
    }

    fn result_type(&self, ty: AstResultTypeKind) -> String {
        match ty {
            AstResultTypeKind::Value(value) => self.value_type(value),
            AstResultTypeKind::Void => "Void".to_owned(),
        }
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
