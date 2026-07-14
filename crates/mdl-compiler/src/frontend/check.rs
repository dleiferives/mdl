//! Signature collection, semantic checking, and structured flow analysis.

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;
use std::rc::Rc;

use super::FrontendLimits;
use super::ast::{
    AstAssignment, AstBindingKind, AstBlock, AstCall, AstCallStatement, AstComparisonOp,
    AstDeclaration, AstExpression, AstExpressionKind, AstFunction, AstIfStatement, AstModule,
    AstResultTypeKind, AstReturnStatement, AstStatement, AstValueTypeKind,
};
use super::hir::{
    CheckedFrontendOutput, FunctionResult, HirBinding, HirBindingKind, HirBlock, HirCall,
    HirComparisonOp, HirExpression, HirExpressionKind, HirFunction, HirIf, HirIfArm, HirModule,
    HirStatement, HirStatementKind, HirVerificationError, LocalId, SourceFunctionId, ValueType,
    verify,
};
use crate::diagnostic::{Diagnostic, DiagnosticLabel, Diagnostics};
use crate::source::{Origin, OriginError, SourceContext, SourceError, Span};

const DUPLICATE_FUNCTION: &str = "frontend.check.duplicate-function";
const DUPLICATE_BINDING: &str = "frontend.check.duplicate-binding";
const UNKNOWN_NAME: &str = "frontend.check.unknown-name";
const TYPE_MISMATCH: &str = "frontend.check.type-mismatch";
const ARGUMENT_COUNT: &str = "frontend.check.argument-count";
const IMMUTABLE_ASSIGNMENT: &str = "frontend.check.immutable-assignment";
const VOID_VALUE: &str = "frontend.check.void-value";
const INTEGER_OUT_OF_RANGE: &str = "frontend.check.integer-out-of-range";
const RETURN_VALUE_REQUIRED: &str = "frontend.check.return-value-required";
const RETURN_VALUE_FORBIDDEN: &str = "frontend.check.return-value-forbidden";
const MISSING_RETURN: &str = "frontend.check.missing-return";
const UNINITIALIZED_READ: &str = "frontend.check.uninitialized-read";
const DIRTY_AST: &str = "frontend.check.dirty-ast";
const TRUNCATED: &str = "frontend.check.truncated";

/// Semantic result: checked HIR on success, or diagnostics without partial HIR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CheckOutput {
    checked: Option<CheckedFrontendOutput>,
    diagnostics: Option<Diagnostics>,
    truncated: bool,
}

impl CheckOutput {
    #[cfg(test)]
    pub(super) const fn checked(&self) -> Option<&CheckedFrontendOutput> {
        self.checked.as_ref()
    }

    #[cfg(test)]
    pub(super) const fn diagnostics(&self) -> Option<&Diagnostics> {
        self.diagnostics.as_ref()
    }

    #[cfg(test)]
    pub(super) const fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub(super) fn into_parts(self) -> (Option<CheckedFrontendOutput>, Option<Diagnostics>) {
        (self.checked, self.diagnostics)
    }
}

/// Closed semantic identity spaces owned by the checked frontend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CheckedEntityKind {
    Function,
    Local,
}

impl fmt::Display for CheckedEntityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Function => formatter.write_str("function"),
            Self::Local => formatter.write_str("local"),
        }
    }
}

/// Infrastructure or invariant failure that prevents semantic output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CheckError {
    Source(SourceError),
    Origin(OriginError),
    IdentitySpaceExhausted(CheckedEntityKind),
    InvalidHir(HirVerificationError),
}

impl fmt::Display for CheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(formatter, "cannot check source: {error}"),
            Self::Origin(error) => write!(formatter, "cannot record semantic origin: {error}"),
            Self::IdentitySpaceExhausted(kind) => {
                write!(formatter, "semantic {kind} identity space is exhausted")
            }
            Self::InvalidHir(error) => write!(formatter, "constructed invalid HIR: {error}"),
        }
    }
}

impl Error for CheckError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Origin(error) => Some(error),
            Self::InvalidHir(error) => Some(error),
            Self::IdentitySpaceExhausted(_) => None,
        }
    }
}

impl From<SourceError> for CheckError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

impl From<OriginError> for CheckError {
    fn from(error: OriginError) -> Self {
        Self::Origin(error)
    }
}

impl From<HirVerificationError> for CheckError {
    fn from(error: HirVerificationError) -> Self {
        Self::InvalidHir(error)
    }
}

/// Checks one parser-clean syntax module.
pub(super) fn check(
    sources: &mut SourceContext,
    module: &AstModule,
    limits: FrontendLimits,
) -> Result<CheckOutput, CheckError> {
    let mut diagnostics = DiagnosticSink::new(limits.max_diagnostics());
    let signatures = collect_signatures(sources, module, &mut diagnostics)?;
    let mut functions = Vec::with_capacity(module.functions.len());

    for signature in &signatures.functions {
        let ast = &module.functions[signature.ast_index];
        let checker = BodyChecker::new(
            sources,
            &mut diagnostics,
            &signatures.functions,
            &signatures.by_name,
            signature,
        );
        functions.push(checker.check_function(ast)?);
    }

    let module_origin = sources.add_origin(Origin::Source(module.span))?;
    let checked = CheckedFrontendOutput::new(HirModule {
        functions: functions.into_boxed_slice(),
        origin: module_origin,
    });
    let truncated = diagnostics.truncation.is_some();
    if !diagnostics.has_errors() {
        verify(&checked, sources)?;
        return Ok(CheckOutput {
            checked: Some(checked),
            diagnostics: None,
            truncated: false,
        });
    }

    let diagnostics = materialize_diagnostics(sources, diagnostics)?;
    Ok(CheckOutput {
        checked: None,
        diagnostics,
        truncated,
    })
}

struct FunctionSignature {
    id: SourceFunctionId,
    ast_index: usize,
    name: Box<str>,
    name_span: Span,
    parameters: Box<[ParameterSignature]>,
    result: FunctionResult,
    result_span: Option<Span>,
}

#[derive(Clone, Copy)]
struct ParameterSignature {
    ty: ValueType,
    type_span: Span,
}

struct SignatureIndex {
    functions: Vec<FunctionSignature>,
    by_name: HashMap<Box<str>, FunctionLookup>,
}

#[derive(Clone, Copy)]
enum FunctionLookup {
    Unique(SourceFunctionId),
    Poisoned,
}

fn collect_signatures(
    sources: &SourceContext,
    module: &AstModule,
    diagnostics: &mut DiagnosticSink,
) -> Result<SignatureIndex, CheckError> {
    let mut functions = Vec::with_capacity(module.functions.len());
    let mut by_name = HashMap::with_capacity(module.functions.len());
    let mut first_spans = HashMap::<Box<str>, Span>::with_capacity(module.functions.len());

    for (ast_index, function) in module.functions.iter().enumerate() {
        let id = SourceFunctionId::from_index(ast_index).ok_or(
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::Function),
        )?;
        let name: Box<str> = sources.files().slice(function.name.span)?.into();
        if let Some(original) = first_spans.get(name.as_ref()).copied() {
            diagnostics.push(
                PendingDiagnostic::new(
                    DUPLICATE_FUNCTION,
                    format!("function `{name}` is declared more than once"),
                    function.name.span,
                )
                .primary("duplicate function declaration")
                .support(original, "first declaration is here"),
            );
            by_name.insert(name.clone(), FunctionLookup::Poisoned);
        } else {
            first_spans.insert(name.clone(), function.name.span);
            by_name.insert(name.clone(), FunctionLookup::Unique(id));
        }
        let parameters = function
            .parameters
            .iter()
            .map(|parameter| ParameterSignature {
                ty: value_type(parameter.ty.kind),
                type_span: parameter.ty.span,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let (result, result_span) = function
            .result
            .map_or((FunctionResult::Void, None), |result| {
                (function_result(result.kind), Some(result.span))
            });
        functions.push(FunctionSignature {
            id,
            ast_index,
            name,
            name_span: function.name.span,
            parameters,
            result,
            result_span,
        });
    }

    Ok(SignatureIndex { functions, by_name })
}

struct BodyChecker<'a> {
    sources: &'a mut SourceContext,
    diagnostics: &'a mut DiagnosticSink,
    signatures: &'a [FunctionSignature],
    functions_by_name: &'a HashMap<Box<str>, FunctionLookup>,
    signature: &'a FunctionSignature,
    bindings: Vec<HirBinding>,
    active_bindings: HashMap<Rc<str>, ActiveBinding>,
    scopes: Vec<Vec<ScopeChange>>,
}

impl<'a> BodyChecker<'a> {
    fn new(
        sources: &'a mut SourceContext,
        diagnostics: &'a mut DiagnosticSink,
        signatures: &'a [FunctionSignature],
        functions_by_name: &'a HashMap<Box<str>, FunctionLookup>,
        signature: &'a FunctionSignature,
    ) -> Self {
        Self {
            sources,
            diagnostics,
            signatures,
            functions_by_name,
            signature,
            bindings: vec![],
            active_bindings: HashMap::new(),
            scopes: vec![vec![]],
        }
    }

    fn check_function(mut self, ast: &AstFunction) -> Result<HirFunction, CheckError> {
        let mut assigned = Assigned::default();
        for (index, parameter) in ast.parameters.iter().enumerate() {
            let name: Rc<str> = Rc::from(self.spelling(parameter.name.span)?);
            let active = self.active_binding(name.as_ref());
            let duplicate = active.map(ActiveBinding::original);
            if let Some(original) = &duplicate {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        DUPLICATE_BINDING,
                        format!("binding `{name}` conflicts with an active binding"),
                        parameter.name.span,
                    )
                    .primary("duplicate or shadowing binding")
                    .support(original.name_span, "active binding was declared here"),
                );
            }
            let local = self.allocate_binding(
                HirBindingKind::Parameter,
                self.signature.parameters[index].ty,
                parameter.name.span,
                parameter.ty.span,
                parameter.span,
            )?;
            assigned.set(local, true);
            let binding = ScopeBinding {
                local,
                kind: HirBindingKind::Parameter,
                ty: self.signature.parameters[index].ty,
                name_span: parameter.name.span,
                type_span: parameter.ty.span,
            };
            let state = duplicate.map_or(ActiveBinding::Unique(binding), |original| {
                ActiveBinding::Poisoned(original)
            });
            self.install_active_binding(name, state, local);
        }

        let checked_body = self.check_block(&ast.body, &mut assigned)?;
        if matches!(self.signature.result, FunctionResult::Value(_)) && checked_body.continues {
            let Some(result_span) = self.signature.result_span else {
                return Err(CheckError::InvalidHir(HirVerificationError::new(
                    "value-result signature has no source span",
                )));
            };
            self.diagnostics.push(
                PendingDiagnostic::new(
                    MISSING_RETURN,
                    format!(
                        "function `{}` can finish without returning a value",
                        self.signature.name
                    ),
                    ast.body.closing_brace,
                )
                .primary("this path reaches the end of the function")
                .support(
                    result_span,
                    "this result type requires a value on every path",
                ),
            );
        }

        let name_origin = self.origin(ast.name.span)?;
        let result_origin = self
            .signature
            .result_span
            .map(|span| self.origin(span))
            .transpose()?;
        let function_origin = self.origin(ast.span)?;
        Ok(HirFunction {
            id: self.signature.id,
            name_origin,
            parameter_count: ast.parameters.len(),
            result: self.signature.result,
            result_origin,
            bindings: self.bindings.into_boxed_slice(),
            body: checked_body.block,
            origin: function_origin,
        })
    }

    fn check_block(
        &mut self,
        ast: &AstBlock,
        assigned: &mut Assigned,
    ) -> Result<CheckedBlock, CheckError> {
        self.scopes.push(vec![]);
        let mut statements = vec![];
        let mut continues = true;
        let mut valid = true;
        for statement in &ast.statements {
            let checked = self.check_statement(statement, assigned)?;
            if let Some(statement) = checked.statement {
                statements.push(statement);
            } else {
                valid = false;
            }
            if continues && !checked.continues {
                continues = false;
            }
        }
        let scope = self.scopes.pop().expect("block scope was pushed");
        for change in scope.into_iter().rev() {
            assigned.set(change.local, false);
            if let Some(previous) = change.previous {
                self.active_bindings.insert(change.name, previous);
            } else {
                self.active_bindings.remove(change.name.as_ref());
            }
        }
        Ok(CheckedBlock {
            block: HirBlock {
                statements: statements.into_boxed_slice(),
                closing_brace_origin: self.origin(ast.closing_brace)?,
                origin: self.origin(ast.span)?,
            },
            continues,
            valid,
        })
    }

    fn check_statement(
        &mut self,
        statement: &AstStatement,
        assigned: &mut Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        match statement {
            AstStatement::Declaration(declaration) => self.check_declaration(declaration, assigned),
            AstStatement::Assignment(assignment) => self.check_assignment(assignment, assigned),
            AstStatement::Call(statement) => self.check_call_statement(statement, assigned),
            AstStatement::If(conditional) => self.check_if(conditional, assigned),
            AstStatement::Return(statement) => self.check_return(statement, assigned),
            AstStatement::Error(span) => {
                self.diagnostics.push(PendingDiagnostic::new(
                    DIRTY_AST,
                    "parser recovery node reached semantic checking",
                    *span,
                ));
                Ok(CheckedStatement::invalid())
            }
        }
    }

    fn check_declaration(
        &mut self,
        declaration: &AstDeclaration,
        assigned: &mut Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let name: Rc<str> = Rc::from(self.spelling(declaration.name.span)?);
        let active = self.active_binding(name.as_ref());
        let duplicate = active.map(ActiveBinding::original);
        if let Some(original) = &duplicate {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    DUPLICATE_BINDING,
                    format!("binding `{name}` conflicts with an active binding"),
                    declaration.name.span,
                )
                .primary("duplicate or shadowing binding")
                .support(original.name_span, "active binding was declared here"),
            );
        }
        let binding_kind = match declaration.kind {
            AstBindingKind::Const => HirBindingKind::Const,
            AstBindingKind::Var => HirBindingKind::Var,
        };
        let declared_type = value_type(declaration.ty.kind);
        let local = self.allocate_binding(
            binding_kind,
            declared_type,
            declaration.name.span,
            declaration.ty.span,
            declaration.span,
        )?;

        let initializer = declaration
            .initializer
            .as_ref()
            .map(|initializer| self.check_expression(initializer, assigned))
            .transpose()?;
        let mut valid = duplicate.is_none();
        if declaration.kind == AstBindingKind::Const && declaration.initializer.is_none() {
            self.diagnostics.push(PendingDiagnostic::new(
                DIRTY_AST,
                "const declaration without an initializer reached semantic checking",
                declaration.span,
            ));
            valid = false;
        }
        if let Some(initializer) = &initializer {
            if let Some(actual) = initializer.ty {
                if actual != declared_type {
                    self.type_mismatch(
                        initializer.span,
                        actual,
                        declared_type,
                        declaration.ty.span,
                        "declared type is here",
                    );
                    valid = false;
                }
            } else {
                valid = false;
            }
            valid &= initializer.expression.is_some();
        }

        assigned.set(local, declaration.initializer.is_some());
        let binding = ScopeBinding {
            local,
            kind: binding_kind,
            ty: declared_type,
            name_span: declaration.name.span,
            type_span: declaration.ty.span,
        };
        let state = duplicate.map_or(ActiveBinding::Unique(binding), |original| {
            ActiveBinding::Poisoned(original)
        });
        self.install_active_binding(name, state, local);

        let initializer = initializer.and_then(|initializer| initializer.expression);
        let kind = valid.then_some(HirStatementKind::Declaration { local, initializer });
        self.finish_statement(kind, declaration.span, true)
    }

    fn check_assignment(
        &mut self,
        assignment: &AstAssignment,
        assigned: &mut Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let name = self.spelling(assignment.target.span)?;
        let target = self.active_binding(name.as_ref());
        if target.is_none() {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNKNOWN_NAME,
                    format!("unknown binding `{name}`"),
                    assignment.target.span,
                )
                .primary("no active binding has this name"),
            );
        }

        let value = self.check_expression(&assignment.value, assigned)?;
        let target = match target {
            Some(ActiveBinding::Unique(target)) => Some(target),
            Some(ActiveBinding::Poisoned(_)) | None => None,
        };
        let mut valid = target.is_some() && value.expression.is_some();
        if let Some(target) = target {
            if !target.kind.is_mutable() {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        IMMUTABLE_ASSIGNMENT,
                        format!("binding `{name}` is immutable"),
                        assignment.target.span,
                    )
                    .primary("cannot assign to this binding")
                    .support(target.name_span, "binding was declared immutable here"),
                );
                valid = false;
            }
            if let Some(actual) = value.ty {
                if actual != target.ty {
                    self.type_mismatch(
                        assignment.value.span,
                        actual,
                        target.ty,
                        target.type_span,
                        "target type is declared here",
                    );
                    valid = false;
                }
            } else {
                valid = false;
            }
            if target.kind.is_mutable() {
                assigned.set(target.local, true);
            }
        }

        let kind = match (valid, target, value.expression) {
            (true, Some(target), Some(value)) => Some(HirStatementKind::Assignment {
                target: target.local,
                value,
            }),
            _ => None,
        };
        self.finish_statement(kind, assignment.span, true)
    }

    fn check_call_statement(
        &mut self,
        statement: &AstCallStatement,
        assigned: &Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let checked = self.check_call(&statement.call, assigned, false)?;
        let kind = checked.call.map(HirStatementKind::Call);
        self.finish_statement(kind, statement.span, true)
    }

    fn check_if(
        &mut self,
        conditional: &AstIfStatement,
        assigned: &mut Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let checkpoint = assigned.checkpoint();
        let mut hir_arms = Vec::with_capacity(conditional.arms.len());
        let mut continuing_delta = None;
        let mut valid = !conditional.arms.is_empty();
        if conditional.arms.is_empty() {
            self.diagnostics.push(PendingDiagnostic::new(
                DIRTY_AST,
                "conditional without an arm reached semantic checking",
                conditional.span,
            ));
        }

        for arm in &conditional.arms {
            let condition = self.check_expression(&arm.condition, assigned)?;
            let condition_is_bool = match condition.ty {
                Some(ValueType::Bool) => true,
                Some(actual) => {
                    self.type_mismatch_without_support(
                        arm.condition.span,
                        actual,
                        ValueType::Bool,
                        "conditions must have type Bool",
                    );
                    false
                }
                None => false,
            };
            let body = self.check_block(&arm.body, assigned)?;
            if body.continues {
                let delta = assigned.newly_assigned_since(checkpoint);
                Assigned::merge_delta_intersection(&mut continuing_delta, &delta);
            }
            assigned.rollback(checkpoint);
            if let (true, Some(condition), true) =
                (condition_is_bool, condition.expression, body.valid)
            {
                hir_arms.push(HirIfArm {
                    condition,
                    body: body.block,
                    origin: self.origin(arm.span)?,
                });
            } else {
                valid = false;
            }
        }

        let else_body = if let Some(ast_else) = &conditional.else_body {
            let body = self.check_block(ast_else, assigned)?;
            if body.continues {
                let delta = assigned.newly_assigned_since(checkpoint);
                Assigned::merge_delta_intersection(&mut continuing_delta, &delta);
            }
            assigned.rollback(checkpoint);
            valid &= body.valid;
            Some(body.block)
        } else {
            Assigned::merge_delta_intersection(&mut continuing_delta, &[]);
            None
        };

        let continues = continuing_delta.is_some();
        if let Some(continuing_delta) = continuing_delta {
            for index in continuing_delta {
                assigned.set_index(index, true);
            }
        }
        valid &= hir_arms.len() == conditional.arms.len();
        let kind = if valid {
            Some(HirStatementKind::If(HirIf {
                arms: hir_arms.into_boxed_slice(),
                else_body,
                origin: self.origin(conditional.span)?,
            }))
        } else {
            None
        };
        if kind.is_none() {
            return Ok(CheckedStatement {
                statement: None,
                continues,
            });
        }
        self.finish_statement(kind, conditional.span, continues)
    }

    fn check_return(
        &mut self,
        statement: &AstReturnStatement,
        assigned: &Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let value = statement
            .value
            .as_ref()
            .map(|value| self.check_expression(value, assigned))
            .transpose()?;
        let mut valid = true;
        match (self.signature.result, &value) {
            (FunctionResult::Void, None) => {}
            (FunctionResult::Void, Some(value)) => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        RETURN_VALUE_FORBIDDEN,
                        "Void function cannot return a value",
                        value.span,
                    )
                    .primary("remove this return value")
                    .support(
                        self.signature
                            .result_span
                            .unwrap_or(self.signature.name_span),
                        "function is declared to return Void",
                    ),
                );
                valid = false;
            }
            (FunctionResult::Value(_), None) => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        RETURN_VALUE_REQUIRED,
                        "value-returning function requires a return value",
                        statement.span,
                    )
                    .primary("this return has no value")
                    .support(
                        self.signature
                            .result_span
                            .expect("value results have explicit source spans"),
                        "required result type is declared here",
                    ),
                );
                valid = false;
            }
            (FunctionResult::Value(expected), Some(value)) => {
                if let Some(actual) = value.ty {
                    if actual != expected {
                        self.type_mismatch(
                            value.span,
                            actual,
                            expected,
                            self.signature
                                .result_span
                                .expect("value results have explicit source spans"),
                            "function result type is declared here",
                        );
                        valid = false;
                    }
                } else {
                    valid = false;
                }
                valid &= value.expression.is_some();
            }
        }
        let value = value.and_then(|value| value.expression);
        let kind = valid.then_some(HirStatementKind::Return(value));
        self.finish_statement(kind, statement.span, false)
    }

    fn check_expression(
        &mut self,
        expression: &AstExpression,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        match &expression.kind {
            AstExpressionKind::Bool(value) => Ok(CheckedExpression::valid(
                HirExpressionKind::Bool(*value),
                ValueType::Bool,
                self.origin(expression.span)?,
                expression.span,
            )),
            AstExpressionKind::DecimalInteger(literal_span) => {
                self.check_decimal(*literal_span, expression.span)
            }
            AstExpressionKind::Name(name) => {
                let spelling = self.spelling(name.span)?;
                let binding = match self.active_binding(spelling.as_ref()) {
                    Some(ActiveBinding::Unique(binding)) => binding,
                    Some(ActiveBinding::Poisoned(_)) => {
                        return Ok(CheckedExpression::invalid(expression.span));
                    }
                    None => {
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                UNKNOWN_NAME,
                                format!("unknown binding `{spelling}`"),
                                name.span,
                            )
                            .primary("no active binding has this name"),
                        );
                        return Ok(CheckedExpression::invalid(expression.span));
                    }
                };
                if !assigned.get(binding.local) {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            UNINITIALIZED_READ,
                            format!("binding `{spelling}` may be uninitialized"),
                            name.span,
                        )
                        .primary("read requires a value on every continuing path")
                        .support(binding.name_span, "binding is declared here"),
                    );
                }
                Ok(CheckedExpression::valid(
                    HirExpressionKind::Local(binding.local),
                    binding.ty,
                    self.origin(expression.span)?,
                    expression.span,
                ))
            }
            AstExpressionKind::Call(call) => {
                let checked = self.check_call(call, assigned, true)?;
                match (checked.call, checked.result) {
                    (Some(call), Some(FunctionResult::Value(ty))) => Ok(CheckedExpression::valid(
                        HirExpressionKind::Call(call),
                        ty,
                        self.origin(expression.span)?,
                        expression.span,
                    )),
                    _ => Ok(CheckedExpression::invalid(expression.span)),
                }
            }
            AstExpressionKind::Not(operand) => {
                let operand = self.check_expression(operand, assigned)?;
                if let Some(actual) = operand.ty {
                    if actual != ValueType::Bool {
                        self.type_mismatch_without_support(
                            operand.span,
                            actual,
                            ValueType::Bool,
                            "`!` requires a Bool operand",
                        );
                        return Ok(CheckedExpression::invalid(expression.span));
                    }
                } else {
                    return Ok(CheckedExpression::invalid(expression.span));
                }
                let Some(operand) = operand.expression else {
                    return Ok(CheckedExpression::invalid(expression.span));
                };
                Ok(CheckedExpression::valid(
                    HirExpressionKind::Not(Box::new(operand)),
                    ValueType::Bool,
                    self.origin(expression.span)?,
                    expression.span,
                ))
            }
            AstExpressionKind::Compare { op, left, right } => {
                self.check_comparison(*op, left, right, expression.span, assigned)
            }
            AstExpressionKind::Error => {
                self.diagnostics.push(PendingDiagnostic::new(
                    DIRTY_AST,
                    "parser recovery expression reached semantic checking",
                    expression.span,
                ));
                Ok(CheckedExpression::invalid(expression.span))
            }
        }
    }

    fn check_decimal(
        &mut self,
        literal_span: Span,
        expression_span: Span,
    ) -> Result<CheckedExpression, CheckError> {
        let spelling = self.sources.files().slice(literal_span)?;
        if let Ok(value) = spelling.parse::<i32>() {
            return Ok(CheckedExpression::valid(
                HirExpressionKind::Int32(value),
                ValueType::Int32,
                self.origin(expression_span)?,
                expression_span,
            ));
        }
        self.diagnostics.push(
            PendingDiagnostic::new(
                INTEGER_OUT_OF_RANGE,
                "decimal integer is outside the Int32 range",
                literal_span,
            )
            .primary("expected a value from 0 through 2147483647"),
        );
        Ok(CheckedExpression::invalid(expression_span))
    }

    fn check_comparison(
        &mut self,
        op: AstComparisonOp,
        left: &AstExpression,
        right: &AstExpression,
        span: Span,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        let left = self.check_expression(left, assigned)?;
        let right = self.check_expression(right, assigned)?;
        let mut valid = left.expression.is_some() && right.expression.is_some();
        let hir_op = comparison_op(op);
        if hir_op.is_ordered() {
            if let Some(actual) = left.ty {
                if actual != ValueType::Int32 {
                    self.type_mismatch_without_support(
                        left.span,
                        actual,
                        ValueType::Int32,
                        "ordered comparisons require Int32 operands",
                    );
                    valid = false;
                }
            } else {
                valid = false;
            }
            if let Some(actual) = right.ty {
                if actual != ValueType::Int32 {
                    self.type_mismatch_without_support(
                        right.span,
                        actual,
                        ValueType::Int32,
                        "ordered comparisons require Int32 operands",
                    );
                    valid = false;
                }
            } else {
                valid = false;
            }
        } else {
            match (left.ty, right.ty) {
                (Some(left_ty), Some(right_ty)) if left_ty != right_ty => {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            TYPE_MISMATCH,
                            format!(
                                "comparison operands have different types: {left_ty} and {right_ty}"
                            ),
                            right.span,
                        )
                        .primary(format!("has type {right_ty}"))
                        .support(left.span, format!("left operand has type {left_ty}")),
                    );
                    valid = false;
                }
                (Some(_), Some(_)) => {}
                _ => valid = false,
            }
        }
        let kind = match (valid, left.expression, right.expression) {
            (true, Some(left), Some(right)) => HirExpressionKind::Compare {
                op: hir_op,
                left: Box::new(left),
                right: Box::new(right),
            },
            _ => return Ok(CheckedExpression::invalid(span)),
        };
        Ok(CheckedExpression::valid(
            kind,
            ValueType::Bool,
            self.origin(span)?,
            span,
        ))
    }

    fn check_call(
        &mut self,
        call: &AstCall,
        assigned: &Assigned,
        require_value: bool,
    ) -> Result<CheckedCall, CheckError> {
        let name = self.spelling(call.callee.span)?;
        let lookup = self.functions_by_name.get(name.as_ref()).copied();
        let signature_index = match lookup {
            Some(FunctionLookup::Unique(id)) => id
                .as_usize()
                .filter(|index| self.signatures.get(*index).is_some()),
            Some(FunctionLookup::Poisoned) | None => None,
        };
        if lookup.is_none() {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNKNOWN_NAME,
                    format!("unknown function `{name}`"),
                    call.callee.span,
                )
                .primary("no function has this name"),
            );
        }

        let mut valid = signature_index.is_some();
        if let Some(signature) = signature_index.and_then(|index| self.signatures.get(index)) {
            if call.arguments.len() != signature.parameters.len() {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        ARGUMENT_COUNT,
                        format!(
                            "function `{name}` expects {} arguments but received {}",
                            signature.parameters.len(),
                            call.arguments.len()
                        ),
                        call.span,
                    )
                    .primary("argument count does not match the function signature")
                    .support(signature.name_span, "function is declared here"),
                );
                valid = false;
            }
            if require_value && signature.result == FunctionResult::Void {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        VOID_VALUE,
                        format!("Void function `{name}` cannot be used as an expression"),
                        call.span,
                    )
                    .primary("this call does not produce a value")
                    .support(
                        signature.result_span.unwrap_or(signature.name_span),
                        "function result is Void",
                    ),
                );
                valid = false;
            }
        }

        let mut arguments = Vec::with_capacity(call.arguments.len());
        for (index, argument) in call.arguments.iter().enumerate() {
            let expected = signature_index
                .and_then(|signature| self.signatures.get(signature))
                .and_then(|signature| signature.parameters.get(index))
                .copied();
            let checked = self.check_expression(argument, assigned)?;
            if let (Some(actual), Some(expected)) = (checked.ty, expected) {
                if actual != expected.ty {
                    self.type_mismatch(
                        argument.span,
                        actual,
                        expected.ty,
                        expected.type_span,
                        "parameter type is declared here",
                    );
                    valid = false;
                }
            }
            if let Some(expression) = checked.expression {
                arguments.push(expression);
            } else {
                valid = false;
            }
        }

        let result = signature_index.map(|signature| self.signatures[signature].result);
        let call = match (valid, signature_index) {
            (true, Some(signature)) => Some(HirCall {
                callee: self.signatures[signature].id,
                arguments: arguments.into_boxed_slice(),
                origin: self.origin(call.span)?,
            }),
            _ => None,
        };
        Ok(CheckedCall { call, result })
    }

    fn allocate_binding(
        &mut self,
        kind: HirBindingKind,
        ty: ValueType,
        name_span: Span,
        type_span: Span,
        span: Span,
    ) -> Result<LocalId, CheckError> {
        let id = LocalId::from_index(self.bindings.len())
            .ok_or(CheckError::IdentitySpaceExhausted(CheckedEntityKind::Local))?;
        let name_origin = self.origin(name_span)?;
        let type_origin = self.origin(type_span)?;
        let origin = self.origin(span)?;
        self.bindings.push(HirBinding {
            id,
            kind,
            ty,
            name_origin,
            type_origin,
            origin,
        });
        Ok(id)
    }

    fn active_binding(&self, name: &str) -> Option<ActiveBinding> {
        self.active_bindings.get(name).copied()
    }

    fn install_active_binding(&mut self, name: Rc<str>, binding: ActiveBinding, local: LocalId) {
        let previous = self.active_bindings.insert(Rc::clone(&name), binding);
        self.scopes
            .last_mut()
            .expect("a function or block scope is active")
            .push(ScopeChange {
                name,
                previous,
                local,
            });
    }

    fn finish_statement(
        &mut self,
        kind: Option<HirStatementKind>,
        span: Span,
        continues: bool,
    ) -> Result<CheckedStatement, CheckError> {
        let statement = if let Some(kind) = kind {
            Some(HirStatement {
                kind,
                origin: self.origin(span)?,
            })
        } else {
            None
        };
        Ok(CheckedStatement {
            statement,
            continues,
        })
    }

    fn type_mismatch(
        &mut self,
        span: Span,
        actual: ValueType,
        expected: ValueType,
        expected_span: Span,
        support_message: &'static str,
    ) {
        self.diagnostics.push(
            PendingDiagnostic::new(
                TYPE_MISMATCH,
                format!("expected {expected}, found {actual}"),
                span,
            )
            .primary(format!("has type {actual}"))
            .support(expected_span, support_message),
        );
    }

    fn type_mismatch_without_support(
        &mut self,
        span: Span,
        actual: ValueType,
        expected: ValueType,
        primary_message: &'static str,
    ) {
        self.diagnostics.push(
            PendingDiagnostic::new(
                TYPE_MISMATCH,
                format!("expected {expected}, found {actual}"),
                span,
            )
            .primary(primary_message),
        );
    }

    fn spelling(&self, span: Span) -> Result<Box<str>, SourceError> {
        self.sources.files().slice(span).map(Into::into)
    }

    fn origin(&mut self, span: Span) -> Result<crate::source::OriginId, OriginError> {
        self.sources.add_origin(Origin::Source(span))
    }
}

#[derive(Clone, Copy)]
struct ScopeBinding {
    local: LocalId,
    kind: HirBindingKind,
    ty: ValueType,
    name_span: Span,
    type_span: Span,
}

#[derive(Clone, Copy)]
enum ActiveBinding {
    Unique(ScopeBinding),
    Poisoned(ScopeBinding),
}

impl ActiveBinding {
    const fn original(self) -> ScopeBinding {
        match self {
            Self::Unique(binding) | Self::Poisoned(binding) => binding,
        }
    }
}

struct ScopeChange {
    name: Rc<str>,
    previous: Option<ActiveBinding>,
    local: LocalId,
}

struct CheckedBlock {
    block: HirBlock,
    continues: bool,
    valid: bool,
}

struct CheckedStatement {
    statement: Option<HirStatement>,
    continues: bool,
}

impl CheckedStatement {
    const fn invalid() -> Self {
        Self {
            statement: None,
            continues: true,
        }
    }
}

struct CheckedExpression {
    expression: Option<HirExpression>,
    ty: Option<ValueType>,
    span: Span,
}

impl CheckedExpression {
    fn valid(
        kind: HirExpressionKind,
        ty: ValueType,
        origin: crate::source::OriginId,
        span: Span,
    ) -> Self {
        Self {
            expression: Some(HirExpression { kind, ty, origin }),
            ty: Some(ty),
            span,
        }
    }

    const fn invalid(span: Span) -> Self {
        Self {
            expression: None,
            ty: None,
            span,
        }
    }
}

struct CheckedCall {
    call: Option<HirCall>,
    result: Option<FunctionResult>,
}

#[derive(Default)]
struct Assigned {
    values: Vec<bool>,
    changes: Vec<AssignedChange>,
}

#[derive(Clone, Copy)]
struct AssignedChange {
    index: usize,
    previous: bool,
}

impl Assigned {
    fn get(&self, local: LocalId) -> bool {
        usize::try_from(local.index())
            .ok()
            .and_then(|index| self.values.get(index))
            .copied()
            .unwrap_or(false)
    }

    fn set(&mut self, local: LocalId, value: bool) {
        let Ok(index) = usize::try_from(local.index()) else {
            return;
        };
        self.set_index(index, value);
    }

    fn set_index(&mut self, index: usize, value: bool) {
        if self.values.len() <= index {
            self.values.resize(index + 1, false);
        }
        if self.values[index] == value {
            return;
        }
        self.changes.push(AssignedChange {
            index,
            previous: self.values[index],
        });
        self.values[index] = value;
    }

    fn checkpoint(&self) -> usize {
        self.changes.len()
    }

    fn rollback(&mut self, checkpoint: usize) {
        while self.changes.len() > checkpoint {
            let change = self
                .changes
                .pop()
                .expect("the change log is longer than the checkpoint");
            self.values[change.index] = change.previous;
        }
    }

    fn newly_assigned_since(&self, checkpoint: usize) -> Vec<usize> {
        let mut initial = BTreeMap::new();
        for change in &self.changes[checkpoint..] {
            initial.entry(change.index).or_insert(change.previous);
        }
        initial
            .into_iter()
            .filter_map(|(index, previous)| {
                (!previous && self.values.get(index) == Some(&true)).then_some(index)
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

struct DiagnosticSink {
    max_diagnostics: usize,
    findings: Vec<PendingDiagnostic>,
    truncation: Option<PendingDiagnostic>,
}

impl DiagnosticSink {
    fn new(max_diagnostics: usize) -> Self {
        Self {
            max_diagnostics,
            findings: vec![],
            truncation: None,
        }
    }

    fn push(&mut self, diagnostic: PendingDiagnostic) {
        if self.truncation.is_some() {
            return;
        }
        if self.findings.len() < self.max_diagnostics {
            self.findings.push(diagnostic);
        } else {
            self.truncation = Some(
                PendingDiagnostic::new(
                    TRUNCATED,
                    "additional semantic diagnostics were omitted",
                    diagnostic.primary_span,
                )
                .primary("semantic diagnostic limit reached here"),
            );
        }
    }

    fn has_errors(&self) -> bool {
        !self.findings.is_empty() || self.truncation.is_some()
    }
}

struct PendingDiagnostic {
    code: &'static str,
    message: String,
    primary_span: Span,
    primary_message: Option<String>,
    supporting: Vec<PendingLabel>,
}

impl PendingDiagnostic {
    fn new(code: &'static str, message: impl Into<String>, primary_span: Span) -> Self {
        Self {
            code,
            message: message.into(),
            primary_span,
            primary_message: None,
            supporting: vec![],
        }
    }

    fn primary(mut self, message: impl Into<String>) -> Self {
        self.primary_message = Some(message.into());
        self
    }

    fn support(mut self, span: Span, message: impl Into<String>) -> Self {
        self.supporting.push(PendingLabel {
            span,
            message: message.into(),
        });
        self
    }
}

struct PendingLabel {
    span: Span,
    message: String,
}

fn materialize_diagnostics(
    sources: &mut SourceContext,
    diagnostics: DiagnosticSink,
) -> Result<Option<Diagnostics>, OriginError> {
    let mut findings = Vec::with_capacity(
        diagnostics.findings.len() + usize::from(diagnostics.truncation.is_some()),
    );
    for pending in diagnostics
        .findings
        .into_iter()
        .chain(diagnostics.truncation)
    {
        let primary = sources.add_origin(Origin::Source(pending.primary_span))?;
        let mut diagnostic = Diagnostic::new(pending.code, pending.message, primary);
        if let Some(message) = pending.primary_message {
            diagnostic = diagnostic.with_primary_label(message);
        }
        for supporting in pending.supporting {
            let origin = sources.add_origin(Origin::Source(supporting.span))?;
            diagnostic = diagnostic.with_supporting_label(
                DiagnosticLabel::new(origin).with_message(supporting.message),
            );
        }
        findings.push(diagnostic);
    }
    Ok(Diagnostics::from_findings(findings))
}

const fn value_type(kind: AstValueTypeKind) -> ValueType {
    match kind {
        AstValueTypeKind::Bool => ValueType::Bool,
        AstValueTypeKind::Int32 => ValueType::Int32,
    }
}

const fn function_result(kind: AstResultTypeKind) -> FunctionResult {
    match kind {
        AstResultTypeKind::Value(kind) => FunctionResult::Value(value_type(kind)),
        AstResultTypeKind::Void => FunctionResult::Void,
    }
}

const fn comparison_op(op: AstComparisonOp) -> HirComparisonOp {
    match op {
        AstComparisonOp::Equal => HirComparisonOp::Equal,
        AstComparisonOp::NotEqual => HirComparisonOp::NotEqual,
        AstComparisonOp::Less => HirComparisonOp::Less,
        AstComparisonOp::LessEqual => HirComparisonOp::LessEqual,
        AstComparisonOp::Greater => HirComparisonOp::Greater,
        AstComparisonOp::GreaterEqual => HirComparisonOp::GreaterEqual,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ARGUMENT_COUNT, CheckOutput, DIRTY_AST, DUPLICATE_BINDING, DUPLICATE_FUNCTION,
        IMMUTABLE_ASSIGNMENT, INTEGER_OUT_OF_RANGE, MISSING_RETURN, RETURN_VALUE_FORBIDDEN,
        RETURN_VALUE_REQUIRED, TRUNCATED, TYPE_MISMATCH, UNINITIALIZED_READ, UNKNOWN_NAME,
        VOID_VALUE, check,
    };
    use crate::frontend::FrontendLimits;
    use crate::frontend::hir::{FunctionResult, SourceFunctionId, ValueType};
    use crate::frontend::lexer::lex;
    use crate::frontend::parser::parse;
    use crate::source::{FileId, SourceContext, Span};

    fn check_text_with_limits(
        text: &str,
        limits: FrontendLimits,
    ) -> (SourceContext, FileId, CheckOutput) {
        let mut sources = SourceContext::new();
        let file = sources.add_file("test.mdl", text).unwrap();
        let lexed = lex(&mut sources, file, limits).unwrap();
        assert_eq!(
            lexed.diagnostics(),
            None,
            "semantic fixtures must be lexically clean"
        );
        let parsed = parse(&mut sources, file, lexed.tokens(), limits).unwrap();
        assert_eq!(
            parsed.diagnostics(),
            None,
            "semantic fixtures must be syntactically clean"
        );
        let (module, _) = parsed.into_parts();
        let output = check(&mut sources, &module, limits).unwrap();
        (sources, file, output)
    }

    fn check_text(text: &str) -> (SourceContext, FileId, CheckOutput) {
        check_text_with_limits(text, FrontendLimits::DEFAULT)
    }

    fn codes(output: &CheckOutput) -> Vec<&'static str> {
        output
            .diagnostics()
            .map_or(&[][..], |diagnostics| diagnostics.findings())
            .iter()
            .map(crate::diagnostic::Diagnostic::code)
            .collect()
    }

    fn diagnostic_span(sources: &SourceContext, output: &CheckOutput, index: usize) -> Span {
        let diagnostic = &output.diagnostics().unwrap().findings()[index];
        sources.resolve_origin_span(diagnostic.origin()).unwrap()
    }

    fn spelling(sources: &SourceContext, span: Span) -> &str {
        sources.files().slice(span).unwrap()
    }

    #[test]
    fn signatures_are_collected_before_forward_and_recursive_body_calls() {
        let text = r"
fn first(x: Int32) -> Int32 { return second(x); }
fn second(x: Int32) -> Int32 {
    if (x == 0) { return first(x); }
    return x;
}
fn invoke() { first(1); }
";
        let (sources, _, output) = check_text(text);
        assert_eq!(output.diagnostics(), None);
        let checked = output.checked().unwrap();
        assert_eq!(checked.function_count(), 3);
        assert_eq!(
            checked.function_ids().collect::<Vec<_>>(),
            [
                SourceFunctionId::from_index(0).unwrap(),
                SourceFunctionId::from_index(1).unwrap(),
                SourceFunctionId::from_index(2).unwrap(),
            ]
        );
        let first = SourceFunctionId::from_index(0).unwrap();
        assert_eq!(
            checked.function_result(first),
            Some(FunctionResult::Value(ValueType::Int32))
        );
        assert_eq!(checked.parameter_count(first), Some(1));
        assert_eq!(checked.parameter_type(first, 0), Some(ValueType::Int32));
        assert_eq!(checked.parameter_type(first, 1), None);
        assert_eq!(checked.dump(&sources), checked.dump(&sources));
        assert!(checked.dump(&sources).contains("call @1(%0:Int32):Int32"));
    }

    #[test]
    fn duplicate_functions_include_the_original_declaration() {
        let text = "fn same() {} fn same() {}";
        let (sources, _, output) = check_text(text);
        assert_eq!(codes(&output), [DUPLICATE_FUNCTION]);
        assert!(output.checked().is_none());
        let diagnostic = &output.diagnostics().unwrap().findings()[0];
        assert_eq!(
            spelling(&sources, diagnostic_span(&sources, &output, 0)),
            "same"
        );
        assert_eq!(diagnostic.supporting_labels().len(), 1);
        let original = sources
            .resolve_origin_span(diagnostic.supporting_labels()[0].origin())
            .unwrap();
        assert_eq!(spelling(&sources, original), "same");
        assert!(original.start() < diagnostic_span(&sources, &output, 0).start());
    }

    #[test]
    fn duplicate_function_names_poison_dependent_call_checks() {
        let text = r"
fn pick(value: Int32) {}
fn pick(value: Bool) {}
fn use_pick() { pick(true); }
";
        let (_, _, output) = check_text(text);
        assert_eq!(codes(&output), [DUPLICATE_FUNCTION]);
    }

    #[test]
    fn active_shadowing_is_rejected_but_disjoint_branches_can_reuse_names() {
        let (_, _, shadowing) =
            check_text("fn bad(value: Int32) { if (true) { var value: Int32; } return; }");
        assert_eq!(codes(&shadowing), [DUPLICATE_BINDING]);
        assert_eq!(
            shadowing.diagnostics().unwrap().findings()[0]
                .supporting_labels()
                .len(),
            1
        );

        let (_, _, disjoint) = check_text(
            "fn good(condition: Bool) { if (condition) { var value: Int32; } else { var value: Int32; } }",
        );
        assert_eq!(disjoint.diagnostics(), None);
        assert!(disjoint.checked().is_some());
    }

    #[test]
    fn duplicate_bindings_are_scoped_poison_without_dependent_noise() {
        let text = r"
fn bad(condition: Bool) {
    var value: Int32 = 0;
    if (condition) {
        var value: Bool = true;
        value = false;
        value = missing;
    }
    value = 1;
}
";
        let (_, _, output) = check_text(text);
        assert_eq!(codes(&output), [DUPLICATE_BINDING, UNKNOWN_NAME]);
    }

    #[test]
    fn empty_conditional_ast_cannot_silently_produce_hir() {
        let mut sources = SourceContext::new();
        let file = sources
            .add_file("test.mdl", "fn valid() { if (true) {} }")
            .unwrap();
        let lexed = lex(&mut sources, file, FrontendLimits::DEFAULT).unwrap();
        let parsed = parse(&mut sources, file, lexed.tokens(), FrontendLimits::DEFAULT).unwrap();
        let (mut module, diagnostics) = parsed.into_parts();
        assert_eq!(diagnostics, None);
        let crate::frontend::ast::AstStatement::If(conditional) =
            &mut module.functions[0].body.statements[0]
        else {
            unreachable!();
        };
        conditional.arms.clear();

        let output = check(&mut sources, &module, FrontendLimits::DEFAULT).unwrap();
        assert_eq!(codes(&output), [DIRTY_AST]);
        assert!(output.checked().is_none());
    }

    #[test]
    fn declarations_enter_scope_only_after_their_initializer() {
        let (_, _, output) =
            check_text("fn bad() -> Int32 { const value: Int32 = value; return value; }");
        assert_eq!(codes(&output), [UNKNOWN_NAME]);
        assert!(!codes(&output).contains(&UNINITIALIZED_READ));
    }

    #[test]
    fn calls_check_arguments_left_to_right_and_exactly() {
        let text = r"
fn target(number: Int32, flag: Bool) {}
fn bad() { target(true, missing); }
";
        let (sources, _, output) = check_text(text);
        assert_eq!(codes(&output), [TYPE_MISMATCH, UNKNOWN_NAME]);
        let spans: Vec<_> = (0..2)
            .map(|index| diagnostic_span(&sources, &output, index).start())
            .collect();
        assert!(spans[0] < spans[1]);

        let (_, _, arity) = check_text("fn target(number: Int32) {} fn bad() { target(1, true); }");
        assert_eq!(codes(&arity), [ARGUMENT_COUNT]);
    }

    #[test]
    fn value_calls_may_be_discarded_but_void_calls_are_not_values() {
        let (_, _, valid) =
            check_text("fn value() -> Int32 { return 1; } fn discard() { value(); return; }");
        assert_eq!(valid.diagnostics(), None);

        let (_, _, invalid) = check_text("fn notify() {} fn bad() -> Int32 { return notify(); }");
        assert_eq!(codes(&invalid), [VOID_VALUE]);
    }

    #[test]
    fn decimal_conversion_accepts_exactly_the_nonnegative_i32_range() {
        let (_, _, valid) = check_text("fn max() -> Int32 { return 2147483647; }");
        assert_eq!(valid.diagnostics(), None);

        let (sources, _, invalid) = check_text("fn overflow() -> Int32 { return 2147483648; }");
        assert_eq!(codes(&invalid), [INTEGER_OUT_OF_RANGE]);
        assert_eq!(
            spelling(&sources, diagnostic_span(&sources, &invalid, 0)),
            "2147483648"
        );
    }

    #[test]
    fn conditions_prefixes_and_comparisons_enforce_exact_types() {
        let (_, _, valid) = check_text(
            "fn good(flag: Bool, number: Int32) -> Bool { if (!flag) { return number < 2; } return flag == true; }",
        );
        assert_eq!(valid.diagnostics(), None);

        let (_, _, invalid) =
            check_text("fn bad() -> Bool { if (1) { return true < false; } return !1; }");
        assert_eq!(
            codes(&invalid),
            [TYPE_MISMATCH, TYPE_MISMATCH, TYPE_MISMATCH, TYPE_MISMATCH]
        );
    }

    #[test]
    fn parameters_and_consts_are_immutable_and_assignment_reads_rhs_first() {
        let text = r"
fn bad(parameter: Int32) {
    const constant: Int32 = 0;
    parameter = 1;
    constant = 2;
    var local: Int32;
    local = local;
}
";
        let (_, _, output) = check_text(text);
        assert_eq!(
            codes(&output),
            [
                IMMUTABLE_ASSIGNMENT,
                IMMUTABLE_ASSIGNMENT,
                UNINITIALIZED_READ,
            ]
        );
    }

    #[test]
    fn definite_assignment_intersects_only_continuing_paths() {
        let text = r"
fn both(condition: Bool) -> Int32 {
    var result: Int32;
    if (condition) { result = 1; } else { result = 2; }
    return result;
}
fn returning(condition: Bool) -> Int32 {
    var result: Int32;
    if (condition) { return 1; } else { result = 2; }
    return result;
}
";
        let (_, _, output) = check_text(text);
        assert_eq!(output.diagnostics(), None);

        let (_, _, missing_else) = check_text(
            "fn bad(condition: Bool) -> Int32 { var result: Int32; if (condition) { result = 1; } return result; }",
        );
        assert_eq!(codes(&missing_else), [UNINITIALIZED_READ]);
    }

    #[test]
    fn all_terminating_arms_satisfy_return_completeness() {
        let (_, _, output) = check_text(
            "fn choose(condition: Bool) -> Int32 { if (condition) { return 1; } else { return 2; } }",
        );
        assert_eq!(output.diagnostics(), None);
        assert!(output.checked().is_some());
    }

    #[test]
    fn missing_return_labels_the_closing_brace_and_result_contract() {
        let text = "fn bad(condition: Bool) -> Int32 { if (condition) { return 1; } }";
        let (sources, _, output) = check_text(text);
        assert_eq!(codes(&output), [MISSING_RETURN]);
        let diagnostic = &output.diagnostics().unwrap().findings()[0];
        assert_eq!(
            spelling(&sources, diagnostic_span(&sources, &output, 0)),
            "}"
        );
        assert_eq!(diagnostic.supporting_labels().len(), 1);
        let result = sources
            .resolve_origin_span(diagnostic.supporting_labels()[0].origin())
            .unwrap();
        assert_eq!(spelling(&sources, result), "-> Int32");

        let (_, _, void_fallthrough) = check_text("fn okay() {}");
        assert_eq!(void_fallthrough.diagnostics(), None);
    }

    #[test]
    fn return_statements_match_the_exact_function_contract() {
        let (_, _, output) = check_text(
            "fn empty() -> Int32 { return; } fn valued() { return 1; } fn wrong() -> Bool { return 1; }",
        );
        assert_eq!(
            codes(&output),
            [RETURN_VALUE_REQUIRED, RETURN_VALUE_FORBIDDEN, TYPE_MISMATCH,]
        );
    }

    #[test]
    fn unreachable_statements_are_checked_without_reopening_function_flow() {
        let (_, _, output) = check_text(
            "fn bad() -> Int32 { return 0; var value: Int32; if (true) { value = true; } return value; }",
        );
        assert_eq!(codes(&output), [TYPE_MISMATCH, UNINITIALIZED_READ]);
        assert!(!codes(&output).contains(&MISSING_RETURN));
    }

    #[test]
    fn diagnostic_limit_retains_one_final_truncation_finding() {
        let limits = FrontendLimits::new(1_000, 32, 2).unwrap();
        let (_, _, output) = check_text_with_limits(
            "fn bad() { missing(); absent(); unknown(); nowhere(); }",
            limits,
        );
        assert_eq!(codes(&output), [UNKNOWN_NAME, UNKNOWN_NAME, TRUNCATED]);
        assert!(output.is_truncated());
        assert_eq!(output.diagnostics().unwrap().len(), 3);
    }

    #[test]
    fn semantic_dump_is_stable_and_contains_resolved_ids_and_types() {
        let text = "fn id(value: Int32) -> Int32 { var copy: Int32 = value; return copy; }";
        let (first_sources, _, first) = check_text(text);
        let (second_sources, _, second) = check_text(text);
        let first = first.checked().unwrap().dump(&first_sources);
        let second = second.checked().unwrap().dump(&second_sources);
        assert_eq!(first, second);
        assert!(first.contains("fn @0 id(%0 value: Int32) -> Int32"));
        assert!(first.contains("%1 var copy: Int32"));
        assert!(first.contains("declare %1 = %0:Int32"));
        assert!(first.contains("return %1:Int32"));
    }

    #[test]
    fn thousands_of_active_bindings_resolve_with_bounded_lookup_state() {
        use std::fmt::Write as _;

        let mut text = String::from("fn scale() {");
        for index in 0..4_096 {
            write!(text, "var local{index}: Int32;").unwrap();
        }
        text.push('}');

        let (_, _, output) = check_text(&text);
        assert_eq!(output.diagnostics(), None);
        assert!(output.checked().is_some());
    }
}
