//! Signature collection, semantic checking, and structured flow analysis.

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;
use std::rc::Rc;

use super::FrontendLimits;
use super::ast::{
    AstAssignment, AstBindingKind, AstBlock, AstCall, AstCallStatement, AstComparisonOp,
    AstCoordinateSigil, AstDeclaration, AstExpression, AstExpressionKind, AstFunction,
    AstFunctionVisibility, AstIfStatement, AstModule, AstName, AstResultTypeKind,
    AstReturnStatement, AstRunModifier, AstRunStatement, AstStatement, AstValueTypeKind,
};
use super::context::{apply_run_modifiers, function_entry_context};
use super::hir::{
    CheckedFrontendOutput, FunctionResult, FunctionVisibility, HirBinding, HirBindingKind,
    HirBlock, HirCall, HirComparisonOp, HirContextStep, HirEntityQuery, HirEntityQueryStep,
    HirExecutionContext, HirExecutorCapture, HirExpression, HirExpressionKind, HirExternalOp,
    HirExternalSemantic, HirFunction, HirIf, HirIfArm, HirMinecraftOperationAttributes, HirModule,
    HirModuleInfo, HirRun, HirRunModifier, HirStatement, HirStatementKind, HirVerificationError,
    LocalId, SourceExternalOpId, SourceFunctionId, SourceModuleId, SourceRunId, ValueType, verify,
};
use super::input::{ModuleDependency, ModuleKey};
use crate::diagnostic::{Diagnostic, DiagnosticLabel, Diagnostics};
use crate::ir::command_line::validate_command_line_shape;
use crate::ir::semantic::{
    Axes, ContextFact, DimensionKey, EntityAnchor, EntityCapability, EntityKind, EntityTag,
    ExecutorType, FiniteDecimal, LocalPosition, MAX_PACKAGE_RUN_MODIFIERS,
    MAX_RUN_MODIFIERS_PER_SCOPE, MessageLiteral, PositionSpec, RotationAxis, RotationSpec,
    SemanticType, SourceReceiverRule, StaticEntityQuery, WorldAxis, WorldPosition,
    minecraft_descriptor, minecraft_source_methods, resolve_minecraft_method,
};
use crate::source::{Origin, OriginError, SourceContext, SourceError, Span};

const DUPLICATE_FUNCTION: &str = "frontend.check.duplicate-function";
const DUPLICATE_IMPORT: &str = "frontend.check.duplicate-import";
const TOP_LEVEL_NAME_CONFLICT: &str = "frontend.check.top-level-name-conflict";
const RESERVED_COMPILER_NAME: &str = "frontend.check.reserved-compiler-name";
const UNKNOWN_IMPORT: &str = "frontend.check.unknown-import";
const UNKNOWN_NAMESPACE: &str = "frontend.check.unknown-namespace";
const UNKNOWN_MEMBER: &str = "frontend.check.unknown-member";
const PRIVATE_MEMBER: &str = "frontend.check.private-member";
const DUPLICATE_BINDING: &str = "frontend.check.duplicate-binding";
const UNKNOWN_NAME: &str = "frontend.check.unknown-name";
const UNRESOLVED_MEMBER: &str = "frontend.check.unresolved-member";
const TYPE_MISMATCH: &str = "frontend.check.type-mismatch";
const ARGUMENT_COUNT: &str = "frontend.check.argument-count";
const IMMUTABLE_ASSIGNMENT: &str = "frontend.check.immutable-assignment";
const VOID_VALUE: &str = "frontend.check.void-value";
const INTEGER_OUT_OF_RANGE: &str = "frontend.check.integer-out-of-range";
const RETURN_VALUE_REQUIRED: &str = "frontend.check.return-value-required";
const RETURN_VALUE_FORBIDDEN: &str = "frontend.check.return-value-forbidden";
const MISSING_RETURN: &str = "frontend.check.missing-return";
const UNINITIALIZED_READ: &str = "frontend.check.uninitialized-read";
const INVALID_UNSAFE_COMMAND: &str = "frontend.check.invalid-unsafe-command";
const INVALID_MESSAGE_LITERAL: &str = "frontend.check.invalid-message-literal";
const MESSAGE_LITERAL_REQUIRED: &str = "frontend.check.message-literal-required";
const INVALID_MINECRAFT_METHOD_RECEIVER: &str = "frontend.check.invalid-minecraft-method-receiver";
const INVALID_RUN_MODIFIER: &str = "frontend.check.invalid-run-modifier";
const INVALID_ENTITY_QUERY: &str = "frontend.check.invalid-entity-query";
const INVALID_ENTITY_TAG: &str = "frontend.check.invalid-entity-tag";
const INVALID_QUERY_LIMIT: &str = "frontend.check.invalid-query-limit";
const INVALID_EXECUTOR_CAPTURE: &str = "frontend.check.invalid-executor-capture";
const SCOPED_CAPABILITY_VALUE: &str = "frontend.check.scoped-capability-value";
const RETURN_IN_RUN_SCOPE: &str = "frontend.check.return-in-run-scope";
const LITERAL_CONTEXT_REQUIRED: &str = "frontend.check.literal-context-required";
const UNSUPPORTED_RUN_SCALAR_CAPTURE: &str = "frontend.check.unsupported-run-scalar-capture";
const DIRTY_AST: &str = "frontend.check.dirty-ast";
const TRUNCATED: &str = "frontend.check.truncated";

fn is_reserved_compiler_name(name: &str) -> bool {
    name == "mc" || EntityKind::from_source_name(name).is_some()
}

fn reserved_compiler_name_diagnostic(name: &str, span: Span) -> PendingDiagnostic {
    PendingDiagnostic::new(
        RESERVED_COMPILER_NAME,
        format!("`{name}` is reserved by the compiler's Minecraft semantic namespace"),
        span,
    )
    .primary("choose a different source binding name")
}

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
    Module,
    Function,
    ExternalOperation,
    RunScope,
    Local,
}

impl fmt::Display for CheckedEntityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Module => formatter.write_str("module"),
            Self::Function => formatter.write_str("function"),
            Self::ExternalOperation => formatter.write_str("external operation"),
            Self::RunScope => formatter.write_str("run scope"),
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

/// One canonical driver module paired with its parser-clean syntax.
pub(super) struct PackageAstModule<'a> {
    pub(super) id: SourceModuleId,
    pub(super) key: &'a ModuleKey,
    pub(super) dependencies: &'a [ModuleDependency],
    pub(super) ast: &'a AstModule,
}

/// Checks one parser-clean syntax module through the package implementation.
#[cfg(test)]
pub(super) fn check(
    sources: &mut SourceContext,
    module: &AstModule,
    limits: FrontendLimits,
) -> Result<CheckOutput, CheckError> {
    let module_id = SourceModuleId::from_index(0).ok_or(CheckError::IdentitySpaceExhausted(
        CheckedEntityKind::Module,
    ))?;
    let key = ModuleKey::single_source_root();
    check_package(
        sources,
        &[PackageAstModule {
            id: module_id,
            key: &key,
            dependencies: &[],
            ast: module,
        }],
        module_id,
        limits,
    )
}

/// Checks a complete canonical package after every module parsed successfully.
pub(super) fn check_package(
    sources: &mut SourceContext,
    modules: &[PackageAstModule<'_>],
    root: SourceModuleId,
    limits: FrontendLimits,
) -> Result<CheckOutput, CheckError> {
    let mut diagnostics = DiagnosticSink::new(limits.max_diagnostics());
    let signatures = collect_signatures(sources, modules, &mut diagnostics)?;
    let function_count = modules
        .iter()
        .map(|module| module.ast.functions.len())
        .sum();
    let mut functions = Vec::with_capacity(function_count);
    let mut external_ops = vec![];
    let mut next_run_scope = 0_usize;
    let mut next_run_modifier = 0_usize;

    for signature in &signatures.functions {
        let module_index =
            signature
                .module
                .as_usize()
                .ok_or(CheckError::IdentitySpaceExhausted(
                    CheckedEntityKind::Module,
                ))?;
        let ast = &modules[module_index].ast.functions[signature.ast_index];
        let checker = BodyChecker::new(
            sources,
            &mut diagnostics,
            &signatures,
            signature,
            &mut external_ops,
            &mut next_run_scope,
            &mut next_run_modifier,
        );
        functions.push(checker.check_function(ast)?);
    }

    let mut hir_modules = Vec::with_capacity(modules.len());
    for module in modules {
        hir_modules.push(HirModuleInfo {
            id: module.id,
            key: module.key.clone(),
            origin: sources.add_origin(Origin::Source(module.ast.span))?,
        });
    }
    let root_index = root
        .as_usize()
        .filter(|index| modules.get(*index).is_some())
        .ok_or(CheckError::IdentitySpaceExhausted(
            CheckedEntityKind::Module,
        ))?;
    let package_origin = sources.add_origin(Origin::Source(modules[root_index].ast.span))?;
    let functions = functions.into_boxed_slice();
    let external_ops = external_ops.into_boxed_slice();
    let behaviors =
        super::behavior::infer_function_behaviors(&functions, &external_ops).map_err(|error| {
            HirVerificationError::new(format!(
                "cannot infer checked function behavior summaries: {error}"
            ))
        })?;
    let checked = CheckedFrontendOutput::new(HirModule {
        root,
        modules: hir_modules.into_boxed_slice(),
        external_ops,
        run_scope_count: next_run_scope,
        functions,
        behaviors,
        origin: package_origin,
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
    module: SourceModuleId,
    ast_index: usize,
    name: Box<str>,
    name_span: Span,
    visibility: FunctionVisibility,
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
    by_module: Vec<HashMap<Box<str>, FunctionLookup>>,
    namespaces: Vec<HashMap<Box<str>, NamespaceLookup>>,
}

#[derive(Clone, Copy)]
enum FunctionLookup {
    Unique(SourceFunctionId),
    Poisoned,
}

#[derive(Clone, Copy)]
struct NamespaceBinding {
    module: SourceModuleId,
    binding_span: Span,
}

#[derive(Clone, Copy)]
enum NamespaceLookup {
    Unique(NamespaceBinding),
    Poisoned,
}

struct CollectedImports {
    namespaces: HashMap<Box<str>, NamespaceLookup>,
    first_spans: HashMap<Box<str>, Span>,
}

fn collect_signatures(
    sources: &SourceContext,
    modules: &[PackageAstModule<'_>],
    diagnostics: &mut DiagnosticSink,
) -> Result<SignatureIndex, CheckError> {
    let module_ids = modules
        .iter()
        .map(|module| (module.key.clone(), module.id))
        .collect::<BTreeMap<_, _>>();
    let function_count = modules
        .iter()
        .map(|module| module.ast.functions.len())
        .sum();
    let mut functions = Vec::with_capacity(function_count);
    let mut by_module = Vec::with_capacity(modules.len());
    let mut namespaces = Vec::with_capacity(modules.len());

    for module in modules {
        let imports = collect_imports(sources, module, &module_ids, diagnostics)?;
        let functions_by_name = collect_module_signatures(
            sources,
            module,
            &imports.first_spans,
            &mut functions,
            diagnostics,
        )?;
        namespaces.push(imports.namespaces);
        by_module.push(functions_by_name);
    }

    Ok(SignatureIndex {
        functions,
        by_module,
        namespaces,
    })
}

fn collect_imports(
    sources: &SourceContext,
    module: &PackageAstModule<'_>,
    module_ids: &BTreeMap<ModuleKey, SourceModuleId>,
    diagnostics: &mut DiagnosticSink,
) -> Result<CollectedImports, CheckError> {
    let dependencies = module
        .dependencies
        .iter()
        .filter_map(|dependency| {
            module_ids
                .get(dependency.target())
                .copied()
                .map(|target| (dependency.name().as_str(), target))
        })
        .collect::<HashMap<_, _>>();
    let mut namespaces = HashMap::with_capacity(module.ast.imports.len());
    let mut first_spans = HashMap::<Box<str>, Span>::with_capacity(module.ast.imports.len());
    for import in &module.ast.imports {
        let binding: Box<str> = sources.files().slice(import.binding.span)?.into();
        if is_reserved_compiler_name(&binding) {
            diagnostics.push(reserved_compiler_name_diagnostic(
                &binding,
                import.binding.span,
            ));
            first_spans.insert(binding.clone(), import.binding.span);
            namespaces.insert(binding, NamespaceLookup::Poisoned);
            continue;
        }
        if let Some(original) = first_spans.get(binding.as_ref()).copied() {
            diagnostics.push(
                PendingDiagnostic::new(
                    DUPLICATE_IMPORT,
                    format!("import binding `{binding}` is declared more than once"),
                    import.binding.span,
                )
                .primary("duplicate import binding")
                .support(original, "first binding is here"),
            );
            namespaces.insert(binding, NamespaceLookup::Poisoned);
            continue;
        }
        first_spans.insert(binding.clone(), import.binding.span);
        let Some(dependency) = decode_string_literal(sources, import.dependency)? else {
            diagnostics.push(PendingDiagnostic::new(
                DIRTY_AST,
                "invalid string literal reached import checking",
                import.dependency,
            ));
            namespaces.insert(binding, NamespaceLookup::Poisoned);
            continue;
        };
        let Some(target) = dependencies.get(dependency.as_ref()).copied() else {
            diagnostics.push(
                PendingDiagnostic::new(
                    UNKNOWN_IMPORT,
                    format!("module has no dependency named `{dependency}`"),
                    import.dependency,
                )
                .primary("the driver did not provide this local dependency name"),
            );
            namespaces.insert(binding, NamespaceLookup::Poisoned);
            continue;
        };
        namespaces.insert(
            binding,
            NamespaceLookup::Unique(NamespaceBinding {
                module: target,
                binding_span: import.binding.span,
            }),
        );
    }
    Ok(CollectedImports {
        namespaces,
        first_spans,
    })
}

fn collect_module_signatures(
    sources: &SourceContext,
    module: &PackageAstModule<'_>,
    import_spans: &HashMap<Box<str>, Span>,
    functions: &mut Vec<FunctionSignature>,
    diagnostics: &mut DiagnosticSink,
) -> Result<HashMap<Box<str>, FunctionLookup>, CheckError> {
    let mut by_name = HashMap::with_capacity(module.ast.functions.len());
    let mut first_spans = HashMap::<Box<str>, Span>::with_capacity(module.ast.functions.len());
    for (ast_index, function) in module.ast.functions.iter().enumerate() {
        let id = SourceFunctionId::from_index(functions.len()).ok_or(
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::Function),
        )?;
        let name: Box<str> = sources.files().slice(function.name.span)?.into();
        if is_reserved_compiler_name(&name) {
            diagnostics.push(reserved_compiler_name_diagnostic(&name, function.name.span));
            by_name.insert(name.clone(), FunctionLookup::Poisoned);
        } else if let Some(import_span) = import_spans.get(name.as_ref()).copied() {
            diagnostics.push(
                PendingDiagnostic::new(
                    TOP_LEVEL_NAME_CONFLICT,
                    format!("function `{name}` conflicts with an import binding"),
                    function.name.span,
                )
                .primary("top-level names share one namespace")
                .support(import_span, "import binding is here"),
            );
            by_name.insert(name.clone(), FunctionLookup::Poisoned);
        } else if let Some(original) = first_spans.get(name.as_ref()).copied() {
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
            module: module.id,
            ast_index,
            name,
            name_span: function.name.span,
            visibility: function_visibility(function.visibility),
            parameters,
            result,
            result_span,
        });
    }
    Ok(by_name)
}

fn decode_string_literal(
    sources: &SourceContext,
    span: Span,
) -> Result<Option<Box<str>>, SourceError> {
    let literal = sources.files().slice(span)?;
    let Some(contents) = literal
        .strip_prefix('"')
        .and_then(|literal| literal.strip_suffix('"'))
    else {
        return Ok(None);
    };
    let mut decoded = String::with_capacity(contents.len());
    let mut characters = contents.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        let Some(escaped) = characters.next() else {
            return Ok(None);
        };
        decoded.push(match escaped {
            '"' => '"',
            '\\' => '\\',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            _ => return Ok(None),
        });
    }
    Ok(Some(decoded.into_boxed_str()))
}

struct BodyChecker<'a> {
    sources: &'a mut SourceContext,
    diagnostics: &'a mut DiagnosticSink,
    signatures: &'a SignatureIndex,
    signature: &'a FunctionSignature,
    external_ops: &'a mut Vec<HirExternalOp>,
    next_run_scope: &'a mut usize,
    next_run_modifier: &'a mut usize,
    bindings: Vec<HirBinding>,
    active_bindings: HashMap<Rc<str>, ActiveBinding>,
    active_executor_captures: HashMap<Rc<str>, ExecutorCaptureBinding>,
    execution_context: HirExecutionContext,
    scopes: Vec<Vec<ScopeChange>>,
    run_depth: usize,
    run_binding_floors: Vec<usize>,
}

impl<'a> BodyChecker<'a> {
    fn new(
        sources: &'a mut SourceContext,
        diagnostics: &'a mut DiagnosticSink,
        signatures: &'a SignatureIndex,
        signature: &'a FunctionSignature,
        external_ops: &'a mut Vec<HirExternalOp>,
        next_run_scope: &'a mut usize,
        next_run_modifier: &'a mut usize,
    ) -> Self {
        Self {
            sources,
            diagnostics,
            signatures,
            signature,
            external_ops,
            next_run_scope,
            next_run_modifier,
            bindings: vec![],
            active_bindings: HashMap::new(),
            active_executor_captures: HashMap::new(),
            execution_context: function_entry_context(),
            scopes: vec![vec![]],
            run_depth: 0,
            run_binding_floors: vec![],
        }
    }

    fn check_function(mut self, ast: &AstFunction) -> Result<HirFunction, CheckError> {
        let mut assigned = Assigned::default();
        for (index, parameter) in ast.parameters.iter().enumerate() {
            let name: Rc<str> = Rc::from(self.spelling(parameter.name.span)?);
            let reserved = is_reserved_compiler_name(&name);
            let active = self.active_binding(name.as_ref());
            let duplicate = active.map(ActiveBinding::original);
            if reserved {
                self.diagnostics.push(reserved_compiler_name_diagnostic(
                    &name,
                    parameter.name.span,
                ));
            } else if let Some(original) = &duplicate {
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
            let state = if reserved {
                ActiveBinding::Poisoned(binding)
            } else {
                duplicate.map_or(ActiveBinding::Unique(binding), |original| {
                    ActiveBinding::Poisoned(original)
                })
            };
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
            module: self.signature.module,
            visibility: function_visibility(ast.visibility),
            visibility_origin: ast
                .visibility_span
                .map(|span| self.origin(span))
                .transpose()?,
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
            AstStatement::Run(statement) => self.check_run(statement, assigned),
            AstStatement::UnsafeMinecraft(statement) => self.check_unsafe_minecraft(statement),
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

    fn check_unsafe_minecraft(
        &mut self,
        statement: &crate::frontend::ast::AstUnsafeMinecraftStatement,
    ) -> Result<CheckedStatement, CheckError> {
        let Some(command) = decode_string_literal(self.sources, statement.command)? else {
            self.diagnostics.push(PendingDiagnostic::new(
                DIRTY_AST,
                "invalid command string reached semantic checking",
                statement.command,
            ));
            return Ok(CheckedStatement::invalid());
        };
        if let Err(error) = validate_command_line_shape(&command) {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_UNSAFE_COMMAND,
                    format!("invalid unsafe Minecraft command: {error}"),
                    statement.command,
                )
                .primary("this literal is not one valid physical command line"),
            );
            return Ok(CheckedStatement::invalid());
        }
        let id = SourceExternalOpId::from_index(self.external_ops.len()).ok_or(
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::ExternalOperation),
        )?;
        let command_origin = self.origin(statement.command)?;
        let origin = self.origin(statement.span)?;
        self.external_ops.push(HirExternalOp {
            id,
            semantic: HirExternalSemantic::UnsafeMinecraftCommand {
                command,
                command_origin,
            },
            origin,
        });
        Ok(CheckedStatement::continuing(HirStatement {
            kind: HirStatementKind::External(id),
            origin,
        }))
    }

    fn check_run(
        &mut self,
        statement: &AstRunStatement,
        assigned: &mut Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let id = SourceRunId::from_index(*self.next_run_scope).ok_or(
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::RunScope),
        )?;
        *self.next_run_scope = (*self.next_run_scope).saturating_add(1);
        let (modifiers, mut valid) = self.check_run_modifiers(statement, id)?;
        let resulting_context =
            if let Ok(context) = apply_run_modifiers(self.execution_context, id, &modifiers) {
                context
            } else {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_RUN_MODIFIER,
                        "run modifier cannot establish its promised execution context",
                        statement.span,
                    )
                    .primary("invalid typed context transition"),
                );
                valid = false;
                self.execution_context
            };
        let final_executor = match resulting_context.executor() {
            ContextFact::Established { value, by } => Some((*value, *by)),
            ContextFact::Unavailable | ContextFact::Inherited => None,
        };
        let installed_capture = match (statement.capture, final_executor) {
            (Some(capture), None) => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_EXECUTOR_CAPTURE,
                        "run capture requires a modifier chain with a proven executor",
                        capture.span,
                    )
                    .primary("no preceding run modifier establishes an executor"),
                );
                valid = false;
                None
            }
            (Some(capture), Some((kind, proof))) => {
                Some(self.install_executor_capture(capture.span, kind, proof, &mut valid)?)
            }
            (None, _) => None,
        };
        let previous_context = std::mem::replace(&mut self.execution_context, resulting_context);
        let body_result = self.check_run_block(&statement.body, assigned);
        self.execution_context = previous_context;
        if let Some(installed) = &installed_capture {
            self.restore_executor_capture(installed);
        }
        let body = body_result?;
        valid &= body.valid;

        let origin = self.origin(statement.span)?;
        let kind = valid.then_some(HirStatementKind::Run(HirRun {
            id,
            modifiers: modifiers.into_boxed_slice(),
            resulting_context,
            capture: installed_capture.map(|installed| installed.hir),
            body: body.block,
            origin,
        }));
        Ok(CheckedStatement {
            statement: kind.map(|kind| HirStatement { kind, origin }),
            continues: true,
        })
    }

    fn check_run_modifiers(
        &mut self,
        statement: &AstRunStatement,
        run: SourceRunId,
    ) -> Result<(Vec<HirRunModifier>, bool), CheckError> {
        let mut modifiers = Vec::with_capacity(statement.modifiers.len());
        let mut valid = true;
        let package_end = self
            .next_run_modifier
            .saturating_add(statement.modifiers.len());
        if statement.modifiers.len() > MAX_RUN_MODIFIERS_PER_SCOPE {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_RUN_MODIFIER,
                    format!(
                        "run scope has {} modifiers; maximum is {MAX_RUN_MODIFIERS_PER_SCOPE}",
                        statement.modifiers.len()
                    ),
                    statement.span,
                )
                .primary("split this contextual region into smaller nested runs"),
            );
            valid = false;
        }
        if package_end > MAX_PACKAGE_RUN_MODIFIERS {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_RUN_MODIFIER,
                    format!("package has more than {MAX_PACKAGE_RUN_MODIFIERS} run modifiers"),
                    statement.span,
                )
                .primary("package-wide run-modifier budget exceeded"),
            );
            valid = false;
        }
        *self.next_run_modifier = package_end;
        let incoming = self.execution_context;
        for modifier in statement.modifiers.iter().take(MAX_RUN_MODIFIERS_PER_SCOPE) {
            let checked = self.check_run_modifier(modifier)?;
            if let Some(checked) = checked {
                modifiers.push(checked);
                if let Ok(context) = apply_run_modifiers(incoming, run, &modifiers) {
                    self.execution_context = context;
                }
            } else {
                valid = false;
            }
        }
        self.execution_context = incoming;
        Ok((modifiers, valid))
    }

    fn install_executor_capture(
        &mut self,
        span: Span,
        kind: EntityKind,
        proof: HirContextStep,
        valid: &mut bool,
    ) -> Result<InstalledExecutorCapture, CheckError> {
        let name: Rc<str> = Rc::from(self.spelling(span)?);
        let reserved = is_reserved_compiler_name(&name);
        if reserved {
            self.diagnostics
                .push(reserved_compiler_name_diagnostic(&name, span));
            *valid = false;
        }
        let duplicate = if reserved {
            None
        } else {
            self.active_binding(name.as_ref())
                .map(|binding| binding.original().name_span)
                .or_else(|| {
                    self.active_executor_captures
                        .get(name.as_ref())
                        .map(|binding| binding.name_span)
                })
        };
        if let Some(original) = duplicate {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    DUPLICATE_BINDING,
                    format!("executor capture `{name}` conflicts with an active binding"),
                    span,
                )
                .primary("duplicate or shadowing capture")
                .support(original, "active binding was declared here"),
            );
            *valid = false;
        }
        let ty = ExecutorType::new(kind);
        let previous = self.active_executor_captures.insert(
            Rc::clone(&name),
            ExecutorCaptureBinding {
                ty,
                proof,
                name_span: span,
            },
        );
        Ok(InstalledExecutorCapture {
            name,
            previous,
            hir: HirExecutorCapture {
                ty,
                proof,
                name_origin: self.origin(span)?,
            },
        })
    }

    fn restore_executor_capture(&mut self, installed: &InstalledExecutorCapture) {
        if let Some(previous) = installed.previous {
            self.active_executor_captures
                .insert(Rc::clone(&installed.name), previous);
        } else {
            self.active_executor_captures
                .remove(installed.name.as_ref());
        }
    }

    fn check_run_block(
        &mut self,
        block: &AstBlock,
        assigned: &mut Assigned,
    ) -> Result<CheckedBlock, CheckError> {
        let checkpoint = assigned.checkpoint();
        self.run_binding_floors.push(self.bindings.len());
        self.run_depth = self.run_depth.saturating_add(1);
        let body = self.check_block(block, assigned);
        self.run_depth = self.run_depth.saturating_sub(1);
        self.run_binding_floors.pop();
        assigned.rollback(checkpoint);
        body
    }

    #[allow(
        clippy::too_many_lines,
        reason = "closed modifier dispatch keeps source rules in one exhaustive boundary"
    )]
    fn check_run_modifier(
        &mut self,
        modifier: &AstRunModifier,
    ) -> Result<Option<HirRunModifier>, CheckError> {
        let name = self.spelling(modifier.name.span)?;
        match name.as_ref() {
            "as" | "at" => self.check_query_run_modifier(name.as_ref(), modifier),
            "at_executor" => {
                if !modifier.arguments.is_empty() {
                    return Ok(self.invalid_run_modifier_arity(modifier, 0));
                }
                let ContextFact::Established {
                    value: kind,
                    by: proof,
                } = self.execution_context.executor()
                else {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            INVALID_RUN_MODIFIER,
                            "run `.at_executor()` requires a statically proven current executor",
                            modifier.span,
                        )
                        .primary("add `.as(query)` before this modifier"),
                    );
                    return Ok(None);
                };
                Ok(Some(HirRunModifier::AtExecutor {
                    kind: *kind,
                    proof: *proof,
                    origin: self.origin(modifier.span)?,
                }))
            }
            "positioned" => {
                let Some(position) = self.check_position_arguments(modifier)? else {
                    return Ok(None);
                };
                Ok(Some(HirRunModifier::Positioned {
                    position,
                    origin: self.origin(modifier.span)?,
                }))
            }
            "rotated" => {
                let Some(rotation) = self.check_rotation_arguments(modifier)? else {
                    return Ok(None);
                };
                Ok(Some(HirRunModifier::Rotated {
                    rotation,
                    origin: self.origin(modifier.span)?,
                }))
            }
            "in" => {
                let Some(dimension) = self.check_builtin_constant(
                    modifier,
                    "dimension",
                    &["overworld", "the_nether", "the_end"],
                )?
                else {
                    return Ok(None);
                };
                let dimension = match dimension.as_ref() {
                    "overworld" => DimensionKey::Overworld,
                    "the_nether" => DimensionKey::TheNether,
                    "the_end" => DimensionKey::TheEnd,
                    _ => unreachable!(),
                };
                Ok(Some(HirRunModifier::In {
                    dimension,
                    origin: self.origin(modifier.span)?,
                }))
            }
            "anchored" => {
                let Some(anchor) =
                    self.check_builtin_constant(modifier, "anchor", &["feet", "eyes"])?
                else {
                    return Ok(None);
                };
                Ok(Some(HirRunModifier::Anchored {
                    anchor: if anchor.as_ref() == "feet" {
                        EntityAnchor::Feet
                    } else {
                        EntityAnchor::Eyes
                    },
                    origin: self.origin(modifier.span)?,
                }))
            }
            "align" => {
                let Some(axes) = self.check_builtin_constant(
                    modifier,
                    "axes",
                    &["x", "y", "z", "xy", "xz", "yz", "xyz"],
                )?
                else {
                    return Ok(None);
                };
                let bits = axes.bytes().fold(0, |bits, axis| {
                    bits | match axis {
                        b'x' => Axes::X,
                        b'y' => Axes::Y,
                        b'z' => Axes::Z,
                        _ => 0,
                    }
                });
                Ok(Some(HirRunModifier::Align {
                    axes: Axes::new(bits).expect("closed axes spelling is nonempty"),
                    origin: self.origin(modifier.span)?,
                }))
            }
            _ => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_RUN_MODIFIER,
                        format!("run modifier `{name}` is not supported by Stage 7.5"),
                        modifier.name.span,
                    )
                    .primary(
                        "expected as, at, at_executor, positioned, rotated, in, anchored, or align",
                    ),
                );
                Ok(None)
            }
        }
    }

    fn check_query_run_modifier(
        &mut self,
        name: &str,
        modifier: &AstRunModifier,
    ) -> Result<Option<HirRunModifier>, CheckError> {
        let [argument] = modifier.arguments.as_slice() else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_RUN_MODIFIER,
                    format!("run `.{name}` requires exactly one entity query"),
                    modifier.span,
                )
                .primary(format!(
                    "received {} modifier arguments",
                    modifier.arguments.len()
                )),
            );
            return Ok(None);
        };
        let Some(query) = self.check_static_entity_query(argument)? else {
            return Ok(None);
        };
        if name == "as"
            && !query
                .semantic
                .kind()
                .capabilities()
                .contains(EntityCapability::CommandExecutor)
        {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_RUN_MODIFIER,
                    format!(
                        "{} cannot establish a command executor",
                        query.semantic.kind()
                    ),
                    argument.span,
                )
                .primary("query kind lacks the CommandExecutor capability"),
            );
            return Ok(None);
        }
        let origin = self.origin(modifier.span)?;
        Ok(Some(if name == "as" {
            HirRunModifier::As { query, origin }
        } else {
            HirRunModifier::At { query, origin }
        }))
    }

    fn invalid_run_modifier_arity(
        &mut self,
        modifier: &AstRunModifier,
        expected: usize,
    ) -> Option<HirRunModifier> {
        self.diagnostics.push(
            PendingDiagnostic::new(
                INVALID_RUN_MODIFIER,
                format!(
                    "run modifier requires {expected} argument(s), received {}",
                    modifier.arguments.len()
                ),
                modifier.span,
            )
            .primary("invalid modifier arity"),
        );
        None
    }

    fn check_position_arguments(
        &mut self,
        modifier: &AstRunModifier,
    ) -> Result<Option<PositionSpec>, CheckError> {
        if modifier.arguments.len() != 3 {
            self.invalid_run_modifier_arity(modifier, 3);
            return Ok(None);
        }
        self.check_position_slice(&modifier.arguments, modifier.span)
    }

    fn check_position_slice(
        &mut self,
        arguments: &[AstExpression],
        span: Span,
    ) -> Result<Option<PositionSpec>, CheckError> {
        let [x, y, z] = arguments else {
            return Ok(None);
        };
        let Some(x) = self.check_spatial_decimal(x)? else {
            return Ok(None);
        };
        let Some(y) = self.check_spatial_decimal(y)? else {
            return Ok(None);
        };
        let Some(z) = self.check_spatial_decimal(z)? else {
            return Ok(None);
        };
        let components = [x, y, z];
        let local_count = components
            .iter()
            .filter(|(sigil, _)| *sigil == Some(AstCoordinateSigil::Local))
            .count();
        if local_count != 0 && local_count != 3 {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_RUN_MODIFIER,
                    "local `^` coordinates cannot mix with absolute or `~` coordinates",
                    span,
                )
                .primary("use `^` for all three components"),
            );
            return Ok(None);
        }
        if local_count == 3 {
            let [(_, left), (_, up), (_, forward)] = components;
            Ok(Some(PositionSpec::Local(LocalPosition {
                left,
                up,
                forward,
            })))
        } else {
            let [x, y, z] = components.map(|(sigil, value)| match sigil {
                Some(AstCoordinateSigil::Relative) => WorldAxis::Relative(value),
                None => WorldAxis::Absolute(value),
                Some(AstCoordinateSigil::Local) => unreachable!(),
            });
            Ok(Some(PositionSpec::World(WorldPosition { x, y, z })))
        }
    }

    fn check_rotation_arguments(
        &mut self,
        modifier: &AstRunModifier,
    ) -> Result<Option<RotationSpec>, CheckError> {
        let [yaw, pitch] = modifier.arguments.as_slice() else {
            self.invalid_run_modifier_arity(modifier, 2);
            return Ok(None);
        };
        let Some(yaw) = self.check_spatial_decimal(yaw)? else {
            return Ok(None);
        };
        let Some(pitch) = self.check_spatial_decimal(pitch)? else {
            return Ok(None);
        };
        if yaw.0 == Some(AstCoordinateSigil::Local) || pitch.0 == Some(AstCoordinateSigil::Local) {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_RUN_MODIFIER,
                    "rotation components cannot use local `^` coordinates",
                    modifier.span,
                )
                .primary("use absolute angles or `~` relative angles"),
            );
            return Ok(None);
        }
        let axis = |(sigil, value)| match sigil {
            Some(AstCoordinateSigil::Relative) => RotationAxis::Relative(value),
            None => RotationAxis::Absolute(value),
            Some(AstCoordinateSigil::Local) => unreachable!(),
        };
        Ok(Some(RotationSpec {
            yaw: axis(yaw),
            pitch: axis(pitch),
        }))
    }

    fn check_spatial_decimal(
        &mut self,
        expression: &AstExpression,
    ) -> Result<Option<(Option<AstCoordinateSigil>, FiniteDecimal)>, CheckError> {
        let (sigil, negative, digits) = match &expression.kind {
            AstExpressionKind::DecimalInteger(span) => (None, false, Some(*span)),
            AstExpressionKind::StaticDecimal {
                sigil,
                negative,
                digits,
            } => (*sigil, *negative, *digits),
            _ => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_RUN_MODIFIER,
                        "expected a compiler-known decimal coordinate",
                        expression.span,
                    )
                    .primary("runtime expressions are not spatial attributes in Stage 7.5"),
                );
                return Ok(None);
            }
        };
        let digits = digits.map_or_else(
            || Ok::<Box<str>, SourceError>("0".into()),
            |span| self.spelling(span),
        )?;
        let spelling = if negative {
            format!("-{digits}")
        } else {
            digits.into()
        };
        match FiniteDecimal::parse(&spelling) {
            Ok(value) => Ok(Some((sigil, value))),
            Err(error) => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_RUN_MODIFIER,
                        format!("invalid spatial decimal: {error}"),
                        expression.span,
                    )
                    .primary("decimal is outside the Stage 7.5 static grammar or limits"),
                );
                Ok(None)
            }
        }
    }

    fn check_builtin_constant(
        &mut self,
        modifier: &AstRunModifier,
        family: &str,
        accepted: &[&str],
    ) -> Result<Option<Box<str>>, CheckError> {
        let [argument] = modifier.arguments.as_slice() else {
            self.invalid_run_modifier_arity(modifier, 1);
            return Ok(None);
        };
        let AstExpressionKind::Member {
            receiver, member, ..
        } = &argument.kind
        else {
            self.invalid_builtin_constant(argument.span, family, accepted);
            return Ok(None);
        };
        let AstExpressionKind::Member {
            receiver: root,
            member: namespace,
            ..
        } = &receiver.kind
        else {
            self.invalid_builtin_constant(argument.span, family, accepted);
            return Ok(None);
        };
        let AstExpressionKind::Name(root) = root.kind else {
            self.invalid_builtin_constant(argument.span, family, accepted);
            return Ok(None);
        };
        let root_name = self.spelling(root.span)?;
        let namespace_name = self.spelling(namespace.span)?;
        let value = self.spelling(member.span)?;
        if root_name.as_ref() != "mc"
            || namespace_name.as_ref() != family
            || !accepted.contains(&value.as_ref())
        {
            self.invalid_builtin_constant(argument.span, family, accepted);
            return Ok(None);
        }
        Ok(Some(value))
    }

    fn invalid_builtin_constant(&mut self, span: Span, family: &str, accepted: &[&str]) {
        self.diagnostics.push(
            PendingDiagnostic::new(
                INVALID_RUN_MODIFIER,
                format!("expected `mc.{family}.<constant>`"),
                span,
            )
            .primary(format!("supported constants: {}", accepted.join(", "))),
        );
    }

    fn check_static_entity_query(
        &mut self,
        expression: &AstExpression,
    ) -> Result<Option<HirEntityQuery>, CheckError> {
        let AstExpressionKind::Call(call) = &expression.kind else {
            self.invalid_entity_query(
                expression.span,
                "expected a compiler-known `mc.entities(...)` query plan",
            );
            return Ok(None);
        };
        let AstExpressionKind::Member {
            receiver, member, ..
        } = &call.callee.kind
        else {
            self.invalid_entity_query(call.callee.span, "expected a typed query method call");
            return Ok(None);
        };
        let member_name = self.spelling(member.span)?;
        match member_name.as_ref() {
            "entities" => self.check_entity_query_root(receiver, &call.arguments, expression.span),
            "with_tag" => {
                self.check_entity_query_with_tag(receiver, &call.arguments, expression.span)
            }
            "limit" => self.check_entity_query_limit(receiver, &call.arguments, expression.span),
            _ => {
                self.invalid_entity_query(
                    member.span,
                    format!("query method `{member_name}` is not supported by this slice"),
                );
                Ok(None)
            }
        }
    }

    fn check_entity_query_with_tag(
        &mut self,
        receiver: &AstExpression,
        arguments: &[AstExpression],
        span: Span,
    ) -> Result<Option<HirEntityQuery>, CheckError> {
        let Some(mut query) = self.check_static_entity_query(receiver)? else {
            return Ok(None);
        };
        let [argument] = arguments else {
            self.invalid_entity_query(span, "`.with_tag` requires one literal");
            return Ok(None);
        };
        let AstExpressionKind::StringLiteral(literal) = argument.kind else {
            self.invalid_entity_query(argument.span, "`.with_tag` requires a string literal");
            return Ok(None);
        };
        let Some(value) = decode_string_literal(self.sources, literal)? else {
            self.invalid_entity_query(literal, "invalid entity-tag string literal");
            return Ok(None);
        };
        match EntityTag::new(value) {
            Ok(tag) => {
                query.semantic.push_tag(tag.clone());
                query.steps.push(HirEntityQueryStep::WithTag {
                    tag,
                    origin: self.origin(span)?,
                    value_origin: self.origin(literal)?,
                });
                Ok(Some(query))
            }
            Err(error) => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_ENTITY_TAG,
                        format!("invalid entity tag: {error}"),
                        literal,
                    )
                    .primary("tag cannot be emitted as a safe Brigadier word"),
                );
                Ok(None)
            }
        }
    }

    fn check_entity_query_limit(
        &mut self,
        receiver: &AstExpression,
        arguments: &[AstExpression],
        span: Span,
    ) -> Result<Option<HirEntityQuery>, CheckError> {
        let Some(mut query) = self.check_static_entity_query(receiver)? else {
            return Ok(None);
        };
        let [argument] = arguments else {
            self.invalid_entity_query(span, "`.limit` requires one positive integer");
            return Ok(None);
        };
        let AstExpressionKind::DecimalInteger(literal) = argument.kind else {
            self.invalid_entity_query(argument.span, "`.limit` requires a decimal integer literal");
            return Ok(None);
        };
        let spelling = self.sources.files().slice(literal)?;
        let Ok(maximum) = spelling.parse::<u32>() else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_QUERY_LIMIT,
                    "entity-query limit is outside the UInt32 range",
                    literal,
                )
                .primary("expected a positive integer from 1 through 4294967295"),
            );
            return Ok(None);
        };
        match query.semantic.refine_limit(maximum) {
            Ok(()) => {
                let maximum = NonZeroU32::new(maximum)
                    .expect("successful semantic query refinement proved a positive limit");
                query.steps.push(HirEntityQueryStep::Limit {
                    maximum,
                    origin: self.origin(span)?,
                    value_origin: self.origin(literal)?,
                });
                Ok(Some(query))
            }
            Err(error) => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_QUERY_LIMIT,
                        format!("invalid entity-query limit: {error}"),
                        literal,
                    )
                    .primary("query limits must be positive"),
                );
                Ok(None)
            }
        }
    }

    fn check_entity_query_root(
        &mut self,
        receiver: &AstExpression,
        arguments: &[AstExpression],
        span: Span,
    ) -> Result<Option<HirEntityQuery>, CheckError> {
        let AstExpressionKind::Name(namespace) = receiver.kind else {
            self.invalid_entity_query(
                receiver.span,
                "query root must use the compiler namespace `mc`",
            );
            return Ok(None);
        };
        if self.spelling(namespace.span)?.as_ref() != "mc" {
            self.invalid_entity_query(
                namespace.span,
                "query root must use the compiler namespace `mc`",
            );
            return Ok(None);
        }
        let [kind] = arguments else {
            self.invalid_entity_query(span, "`mc.entities` requires one nominal entity kind");
            return Ok(None);
        };
        let AstExpressionKind::Name(kind_name) = kind.kind else {
            self.invalid_entity_query(kind.span, "expected the nominal entity kind `ArmorStand`");
            return Ok(None);
        };
        let spelling = self.spelling(kind_name.span)?;
        let Some(kind) = EntityKind::from_source_name(&spelling) else {
            self.invalid_entity_query(
                kind_name.span,
                format!("entity kind `{spelling}` is not supported by this slice"),
            );
            return Ok(None);
        };
        Ok(Some(HirEntityQuery {
            semantic: StaticEntityQuery::entities(kind),
            steps: vec![HirEntityQueryStep::Entities {
                kind,
                origin: self.origin(span)?,
                kind_origin: self.origin(kind_name.span)?,
            }],
        }))
    }

    fn invalid_entity_query(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(
            PendingDiagnostic::new(INVALID_ENTITY_QUERY, message, span)
                .primary("expected a closed static entity-query plan"),
        );
    }

    fn check_declaration(
        &mut self,
        declaration: &AstDeclaration,
        assigned: &mut Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let name: Rc<str> = Rc::from(self.spelling(declaration.name.span)?);
        let reserved = is_reserved_compiler_name(&name);
        let active = self.active_binding(name.as_ref());
        let duplicate = active.map(ActiveBinding::original);
        let duplicate_span = duplicate.map(|binding| binding.name_span).or_else(|| {
            self.active_executor_captures
                .get(name.as_ref())
                .map(|capture| capture.name_span)
        });
        if reserved {
            self.diagnostics.push(reserved_compiler_name_diagnostic(
                &name,
                declaration.name.span,
            ));
        } else if let Some(original) = duplicate_span {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    DUPLICATE_BINDING,
                    format!("binding `{name}` conflicts with an active binding"),
                    declaration.name.span,
                )
                .primary("duplicate or shadowing binding")
                .support(original, "active binding was declared here"),
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
        let mut valid = duplicate_span.is_none() && !reserved;
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
        let state = if reserved {
            ActiveBinding::Poisoned(binding)
        } else {
            duplicate.map_or(ActiveBinding::Unique(binding), |original| {
                ActiveBinding::Poisoned(original)
            })
        };
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

        let implicit_run_capture = target.is_some_and(|target| {
            let binding = target.original();
            self.run_binding_floors.last().is_some_and(|floor| {
                usize::try_from(binding.local.index()).is_ok_and(|index| index < *floor)
            })
        });
        if implicit_run_capture {
            let binding = target
                .expect("the capture check proved a target")
                .original();
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNSUPPORTED_RUN_SCALAR_CAPTURE,
                    format!("run block cannot access outer scalar `{name}`"),
                    assignment.target.span,
                )
                .primary("ordinary scalar captures are not supported by this Stage 7 slice")
                .support(binding.name_span, "outer binding is declared here"),
            );
        }

        let value = self.check_expression(&assignment.value, assigned)?;
        let target = match (target, implicit_run_capture) {
            (Some(ActiveBinding::Unique(target)), false) => Some(target),
            (_, true) | (Some(ActiveBinding::Poisoned(_)) | None, false) => None,
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
        match checked.target {
            Some(CheckedCallTarget::Function(call)) => {
                self.finish_statement(Some(HirStatementKind::Call(call)), statement.span, true)
            }
            Some(CheckedCallTarget::External(operation)) => {
                let origin = self.origin(statement.span)?;
                let external = self
                    .external_ops
                    .get_mut(
                        operation
                            .as_usize()
                            .expect("allocated external ID fits usize"),
                    )
                    .expect("checked external call was just allocated");
                external.origin = origin;
                Ok(CheckedStatement::continuing(HirStatement {
                    kind: HirStatementKind::External(operation),
                    origin,
                }))
            }
            None => self.finish_statement(None, statement.span, true),
        }
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
        if self.run_depth != 0 {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    RETURN_IN_RUN_SCOPE,
                    "return inside a contextual run block is not defined in Stage 7",
                    statement.span,
                )
                .primary("run blocks are Void contextual regions, not function bodies"),
            );
            return Ok(CheckedStatement::invalid());
        }
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
            AstExpressionKind::StaticDecimal { .. } => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        LITERAL_CONTEXT_REQUIRED,
                        "spatial decimal literals require a compiler-known coordinate context",
                        expression.span,
                    )
                    .primary("this literal is not an ordinary runtime value"),
                );
                Ok(CheckedExpression::invalid(expression.span))
            }
            AstExpressionKind::StringLiteral(_) => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        LITERAL_CONTEXT_REQUIRED,
                        "string literals require a compiler-known typed context",
                        expression.span,
                    )
                    .primary("this literal is not an ordinary runtime string value"),
                );
                Ok(CheckedExpression::invalid(expression.span))
            }
            AstExpressionKind::Name(name) => {
                self.check_name_expression(*name, expression.span, assigned)
            }
            AstExpressionKind::Member {
                receiver, member, ..
            } => self.check_unresolved_member(receiver, member.span, expression.span, assigned),
            AstExpressionKind::Call(call) => {
                let checked = self.check_call(call, assigned, true)?;
                match (checked.target, checked.result) {
                    (Some(CheckedCallTarget::Function(call)), Some(FunctionResult::Value(ty))) => {
                        Ok(CheckedExpression::valid(
                            HirExpressionKind::Call(call),
                            ty,
                            self.origin(expression.span)?,
                            expression.span,
                        ))
                    }
                    _ => Ok(CheckedExpression::invalid(expression.span)),
                }
            }
            AstExpressionKind::Not(operand) => {
                self.check_not_expression(operand, expression.span, assigned)
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

    fn check_name_expression(
        &mut self,
        name: AstName,
        expression_span: Span,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        let spelling = self.spelling(name.span)?;
        let binding = match self.active_binding(spelling.as_ref()) {
            Some(ActiveBinding::Unique(binding)) => binding,
            Some(ActiveBinding::Poisoned(_)) => {
                return Ok(CheckedExpression::invalid(expression_span));
            }
            None => {
                return Ok(self.invalid_name_expression(
                    name.span,
                    expression_span,
                    spelling.as_ref(),
                ));
            }
        };
        if self.run_binding_floors.last().is_some_and(|floor| {
            usize::try_from(binding.local.index()).is_ok_and(|index| index < *floor)
        }) {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNSUPPORTED_RUN_SCALAR_CAPTURE,
                    format!("run block cannot access outer scalar `{spelling}`"),
                    name.span,
                )
                .primary("ordinary scalar captures are not supported by this Stage 7 slice")
                .support(binding.name_span, "outer binding is declared here"),
            );
            return Ok(CheckedExpression::invalid(expression_span));
        }
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
            self.origin(expression_span)?,
            expression_span,
        ))
    }

    fn invalid_name_expression(
        &mut self,
        name_span: Span,
        expression_span: Span,
        spelling: &str,
    ) -> CheckedExpression {
        if let Some(capture) = self.active_executor_captures.get(spelling) {
            let is_current = matches!(
                self.execution_context.executor(),
                ContextFact::Established { by, .. } if *by == capture.proof
            );
            self.diagnostics.push(
                PendingDiagnostic::new(
                    SCOPED_CAPABILITY_VALUE,
                    format!("executor capture `{spelling}` is not an ordinary value"),
                    name_span,
                )
                .primary(if is_current {
                    "use the capture only as a supported method receiver"
                } else {
                    "a nested executor transition replaced the capture's executor proof"
                })
                .support(
                    capture.name_span,
                    format!("capture has type Executor<{}>", capture.ty.kind()),
                ),
            );
        } else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNKNOWN_NAME,
                    format!("unknown binding `{spelling}`"),
                    name_span,
                )
                .primary("no active binding has this name"),
            );
        }
        CheckedExpression::invalid(expression_span)
    }

    fn check_not_expression(
        &mut self,
        operand: &AstExpression,
        expression_span: Span,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        let operand = self.check_expression(operand, assigned)?;
        let Some(actual) = operand.ty else {
            return Ok(CheckedExpression::invalid(expression_span));
        };
        if actual != ValueType::Bool {
            self.type_mismatch_without_support(
                operand.span,
                actual,
                ValueType::Bool,
                "`!` requires a Bool operand",
            );
            return Ok(CheckedExpression::invalid(expression_span));
        }
        let Some(operand) = operand.expression else {
            return Ok(CheckedExpression::invalid(expression_span));
        };
        Ok(CheckedExpression::valid(
            HirExpressionKind::Not(Box::new(operand)),
            ValueType::Bool,
            self.origin(expression_span)?,
            expression_span,
        ))
    }

    fn check_unresolved_member(
        &mut self,
        receiver: &AstExpression,
        member_span: Span,
        expression_span: Span,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        let _ = self.check_expression(receiver, assigned)?;
        let member_name = self.spelling(member_span)?;
        self.diagnostics.push(
            PendingDiagnostic::new(
                UNRESOLVED_MEMBER,
                format!("member `{member_name}` cannot be resolved on this value"),
                member_span,
            )
            .primary("member access requires a known namespace or typed receiver"),
        );
        Ok(CheckedExpression::invalid(expression_span))
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
        if let Some(checked) = self.check_minecraft_method_call(call, assigned, require_value)? {
            return Ok(checked);
        }
        let (name, signature_index) = self.resolve_call_target(&call.callee, assigned)?;

        let mut valid = signature_index.is_some();
        if let Some(signature) =
            signature_index.and_then(|index| self.signatures.functions.get(index))
        {
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
                .and_then(|signature| self.signatures.functions.get(signature))
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

        let result = signature_index.map(|signature| self.signatures.functions[signature].result);
        let target = match (valid, signature_index) {
            (true, Some(signature)) => Some(CheckedCallTarget::Function(HirCall {
                callee: self.signatures.functions[signature].id,
                arguments: arguments.into_boxed_slice(),
                origin: self.origin(call.span)?,
            })),
            _ => None,
        };
        Ok(CheckedCall { target, result })
    }

    #[allow(
        clippy::single_match_else,
        clippy::too_many_lines,
        reason = "typed method checking keeps receiver proof, arity, literal provenance, and recovery in one diagnostic transaction"
    )]
    fn check_minecraft_method_call(
        &mut self,
        call: &AstCall,
        assigned: &Assigned,
        require_value: bool,
    ) -> Result<Option<CheckedCall>, CheckError> {
        let AstExpressionKind::Member {
            receiver, member, ..
        } = &call.callee.kind
        else {
            return Ok(None);
        };
        let member_spelling = self.spelling(member.span)?;
        let required_capability = minecraft_source_methods()
            .iter()
            .find(|rule| rule.name() == member_spelling.as_ref())
            .map(|rule| match rule.receiver() {
                SourceReceiverRule::CurrentExecutor {
                    required_capability,
                } => required_capability,
            });
        let AstExpressionKind::Name(receiver_name) = &receiver.kind else {
            return Ok(None);
        };
        let receiver_spelling = self.spelling(receiver_name.span)?;
        let Some(capture) = self
            .active_executor_captures
            .get(receiver_spelling.as_ref())
            .copied()
        else {
            let (Some(required_capability), Some(binding)) = (
                required_capability,
                self.active_binding(receiver_spelling.as_ref()),
            ) else {
                return Ok(None);
            };
            let binding = binding.original();
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_MINECRAFT_METHOD_RECEIVER,
                    format!(
                        "Minecraft method `{member_spelling}` requires the current \
                         Executor<T: {required_capability}> capture"
                    ),
                    receiver.span,
                )
                .primary(format!(
                    "`{receiver_spelling}` has type {}, not an executor-capture proof",
                    binding.ty
                ))
                .support(binding.name_span, "ordinary value is declared here"),
            );
            self.check_unknown_minecraft_method_arguments(call, assigned)?;
            return Ok(Some(CheckedCall {
                target: None,
                result: Some(FunctionResult::Void),
            }));
        };

        let Some(rule) =
            resolve_minecraft_method(SemanticType::Executor(capture.ty), member_spelling.as_ref())
        else {
            if let Some(required_capability) = required_capability {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_MINECRAFT_METHOD_RECEIVER,
                        format!(
                            "Minecraft method `{member_spelling}` requires the current \
                             Executor<T: {required_capability}> capture"
                        ),
                        receiver.span,
                    )
                    .primary(format!(
                        "Executor<{}> does not provide {required_capability}",
                        capture.ty.kind()
                    ))
                    .support(capture.name_span, "executor capture is established here"),
                );
            } else {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        UNKNOWN_MEMBER,
                        format!(
                            "Executor<{}> has no Minecraft method `{member_spelling}`",
                            capture.ty.kind()
                        ),
                        member.span,
                    )
                    .primary("unknown typed Minecraft method")
                    .support(capture.name_span, "executor capture is established here"),
                );
            }
            self.check_unknown_minecraft_method_arguments(call, assigned)?;
            return Ok(Some(CheckedCall {
                target: None,
                result: Some(FunctionResult::Void),
            }));
        };

        let mut valid = true;
        let is_current = matches!(
            self.execution_context.executor(),
            ContextFact::Established { value, by }
                if *value == capture.ty.kind() && *by == capture.proof
        );
        if !is_current {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    SCOPED_CAPABILITY_VALUE,
                    format!(
                        "executor capture `{receiver_spelling}` no longer proves the current executor"
                    ),
                    receiver.span,
                )
                .primary("a nested executor transition replaced this exact proof")
                .support(capture.name_span, "capture was established here"),
            );
            valid = false;
        }

        let descriptor = minecraft_descriptor(rule.semantic_key());
        let _signature = descriptor.signature();
        let expected_arguments = match rule.semantic_key() {
            crate::ir::semantic::MinecraftSemanticKey::Say => 1,
            crate::ir::semantic::MinecraftSemanticKey::TeleportCurrentExecutor
            | crate::ir::semantic::MinecraftSemanticKey::MoveCurrentExecutorBy => 3,
        };
        if call.arguments.len() != expected_arguments {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    ARGUMENT_COUNT,
                    format!(
                        "Minecraft method `{}` expects {expected_arguments} arguments but received {}",
                        rule.name(),
                        call.arguments.len()
                    ),
                    call.span,
                )
                .primary("argument count does not match the typed Minecraft method"),
            );
            valid = false;
        }
        if require_value {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    VOID_VALUE,
                    format!(
                        "Minecraft method `{}` cannot be used as an expression",
                        rule.name()
                    ),
                    call.span,
                )
                .primary("this typed Minecraft command produces source Void"),
            );
            valid = false;
        }

        let mut attributes = None;
        match rule.semantic_key() {
            crate::ir::semantic::MinecraftSemanticKey::Say => {
                if let Some(argument) = call.arguments.first() {
                    match argument.kind {
                        AstExpressionKind::StringLiteral(literal_span) => {
                            match decode_string_literal(self.sources, literal_span)? {
                                Some(decoded) => match MessageLiteral::new(decoded) {
                                    Ok(message) => {
                                        attributes = Some(HirMinecraftOperationAttributes::Say {
                                            message,
                                            message_origin: self.origin(literal_span)?,
                                        });
                                    }
                                    Err(error) => {
                                        self.diagnostics.push(
                                            PendingDiagnostic::new(
                                                INVALID_MESSAGE_LITERAL,
                                                error.to_string(),
                                                literal_span,
                                            )
                                            .primary("message is outside the typed literal subset"),
                                        );
                                        valid = false;
                                    }
                                },
                                None => {
                                    self.diagnostics.push(PendingDiagnostic::new(
                                        DIRTY_AST,
                                        "invalid string literal reached Minecraft method checking",
                                        literal_span,
                                    ));
                                    valid = false;
                                }
                            }
                        }
                        _ => {
                            let _ = self.check_expression(argument, assigned)?;
                            self.diagnostics.push(
                                PendingDiagnostic::new(
                                    MESSAGE_LITERAL_REQUIRED,
                                    format!(
                                        "Minecraft method `{}` requires a string literal",
                                        rule.name()
                                    ),
                                    argument.span,
                                )
                                .primary(
                                    "runtime values cannot supply compile-time command attributes",
                                ),
                            );
                            valid = false;
                        }
                    }
                }
            }
            crate::ir::semantic::MinecraftSemanticKey::TeleportCurrentExecutor => {
                if call.arguments.len() == 3 {
                    if let Some(position) = self.check_position_slice(&call.arguments, call.span)? {
                        attributes = Some(HirMinecraftOperationAttributes::Teleport {
                            position,
                            component_origins: [
                                self.origin(call.arguments[0].span)?,
                                self.origin(call.arguments[1].span)?,
                                self.origin(call.arguments[2].span)?,
                            ],
                        });
                    } else {
                        valid = false;
                    }
                }
            }
            crate::ir::semantic::MinecraftSemanticKey::MoveCurrentExecutorBy => {
                if let [x, y, z] = call.arguments.as_slice() {
                    let values = [
                        self.check_spatial_decimal(x)?,
                        self.check_spatial_decimal(y)?,
                        self.check_spatial_decimal(z)?,
                    ];
                    if let [Some((None, x)), Some((None, y)), Some((None, z))] = values {
                        attributes = Some(HirMinecraftOperationAttributes::MoveBy {
                            offset: crate::ir::semantic::RelativeWorldOffset { x, y, z },
                            component_origins: [
                                self.origin(call.arguments[0].span)?,
                                self.origin(call.arguments[1].span)?,
                                self.origin(call.arguments[2].span)?,
                            ],
                        });
                    } else {
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                INVALID_RUN_MODIFIER,
                                "move_by offsets must be plain exact decimals",
                                call.span,
                            )
                            .primary(
                                "`~` and `^` are implicit in this receiver-relative operation",
                            ),
                        );
                        valid = false;
                    }
                }
            }
        }

        let target = if valid {
            let attributes = attributes.expect("validated Minecraft method has static attributes");
            let id = SourceExternalOpId::from_index(self.external_ops.len()).ok_or(
                CheckError::IdentitySpaceExhausted(CheckedEntityKind::ExternalOperation),
            )?;
            let origin = self.origin(call.span)?;
            let call_origin = origin;
            let member_origin = self.origin(member.span)?;
            let receiver_origin = self.origin(receiver.span)?;
            self.external_ops.push(HirExternalOp {
                id,
                semantic: HirExternalSemantic::MinecraftOperation {
                    key: rule.semantic_key(),
                    receiver_kind: capture.ty.kind(),
                    executor_proof: capture.proof,
                    attributes,
                    call_origin,
                    member_origin,
                    receiver_origin,
                },
                origin,
            });
            Some(CheckedCallTarget::External(id))
        } else {
            None
        };
        Ok(Some(CheckedCall {
            target,
            result: Some(FunctionResult::Void),
        }))
    }

    fn check_unknown_minecraft_method_arguments(
        &mut self,
        call: &AstCall,
        assigned: &Assigned,
    ) -> Result<(), CheckError> {
        for argument in &call.arguments {
            if !matches!(argument.kind, AstExpressionKind::StringLiteral(_)) {
                let _ = self.check_expression(argument, assigned)?;
            }
        }
        Ok(())
    }

    fn resolve_call_target(
        &mut self,
        callee: &AstExpression,
        assigned: &Assigned,
    ) -> Result<(Box<str>, Option<usize>), CheckError> {
        match &callee.kind {
            AstExpressionKind::Name(name) => self.resolve_local_function(name.span),
            AstExpressionKind::Member {
                receiver, member, ..
            } => self.resolve_namespace_member(receiver, member.span, assigned),
            _ => {
                let _ = self.check_expression(callee, assigned)?;
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        UNRESOLVED_MEMBER,
                        "call target is not a function",
                        callee.span,
                    )
                    .primary("expected a local function or imported namespace member"),
                );
                Ok((Box::from("<unresolved>"), None))
            }
        }
    }

    fn resolve_local_function(
        &mut self,
        name_span: Span,
    ) -> Result<(Box<str>, Option<usize>), CheckError> {
        let name = self.spelling(name_span)?;
        let lookup = self
            .signature
            .module
            .as_usize()
            .and_then(|module| self.signatures.by_module.get(module))
            .and_then(|functions| functions.get(name.as_ref()))
            .copied();
        let signature = self.signature_index(lookup);
        if lookup.is_none() {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNKNOWN_NAME,
                    format!("unknown function `{name}`"),
                    name_span,
                )
                .primary("no function in this module has this name"),
            );
        }
        Ok((name, signature))
    }

    fn resolve_namespace_member(
        &mut self,
        receiver: &AstExpression,
        member_span: Span,
        assigned: &Assigned,
    ) -> Result<(Box<str>, Option<usize>), CheckError> {
        let AstExpressionKind::Name(namespace_name) = &receiver.kind else {
            let _ = self.check_expression(receiver, assigned)?;
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNRESOLVED_MEMBER,
                    "member-call receiver is not an imported namespace",
                    receiver.span,
                )
                .primary("nested members and value methods are not available in this tranche"),
            );
            return Ok((Box::from("<unresolved>"), None));
        };
        let namespace = self.spelling(namespace_name.span)?;
        if self.active_binding(namespace.as_ref()).is_some() {
            let _ = self.check_expression(receiver, assigned)?;
            let member = self.spelling(member_span)?;
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNRESOLVED_MEMBER,
                    format!("member `{member}` cannot be called on this value"),
                    member_span,
                )
                .primary("typed value methods are introduced after package namespaces"),
            );
            return Ok((member, None));
        }

        let namespace_lookup = self
            .signature
            .module
            .as_usize()
            .and_then(|module| self.signatures.namespaces.get(module))
            .and_then(|namespaces| namespaces.get(namespace.as_ref()))
            .copied();
        let Some(NamespaceLookup::Unique(namespace_binding)) = namespace_lookup else {
            if namespace_lookup.is_none() {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        UNKNOWN_NAMESPACE,
                        format!("unknown namespace `{namespace}`"),
                        namespace_name.span,
                    )
                    .primary("no import binding has this name"),
                );
            }
            return Ok((self.spelling(member_span)?, None));
        };

        let member = self.spelling(member_span)?;
        let lookup = namespace_binding
            .module
            .as_usize()
            .and_then(|module| self.signatures.by_module.get(module))
            .and_then(|functions| functions.get(member.as_ref()))
            .copied();
        let signature_index = self.signature_index(lookup);
        let Some(signature_index) = signature_index else {
            if lookup.is_none() {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        UNKNOWN_MEMBER,
                        format!("namespace `{namespace}` has no function `{member}`"),
                        member_span,
                    )
                    .primary("unknown namespace member")
                    .support(namespace_binding.binding_span, "namespace is imported here"),
                );
            }
            return Ok((member, None));
        };
        let signature = &self.signatures.functions[signature_index];
        if signature.visibility == FunctionVisibility::Private {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    PRIVATE_MEMBER,
                    format!("function `{member}` is private to its module"),
                    member_span,
                )
                .primary("private function cannot be called through an import")
                .support(signature.name_span, "private function is declared here"),
            );
            return Ok((member, None));
        }
        Ok((member, Some(signature_index)))
    }

    fn signature_index(&self, lookup: Option<FunctionLookup>) -> Option<usize> {
        match lookup {
            Some(FunctionLookup::Unique(id)) => id
                .as_usize()
                .filter(|index| self.signatures.functions.get(*index).is_some()),
            Some(FunctionLookup::Poisoned) | None => None,
        }
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
struct ExecutorCaptureBinding {
    ty: ExecutorType,
    proof: HirContextStep,
    name_span: Span,
}

struct InstalledExecutorCapture {
    name: Rc<str>,
    previous: Option<ExecutorCaptureBinding>,
    hir: HirExecutorCapture,
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
    const fn continuing(statement: HirStatement) -> Self {
        Self {
            statement: Some(statement),
            continues: true,
        }
    }

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
    target: Option<CheckedCallTarget>,
    result: Option<FunctionResult>,
}

enum CheckedCallTarget {
    Function(HirCall),
    External(SourceExternalOpId),
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

const fn function_visibility(visibility: AstFunctionVisibility) -> FunctionVisibility {
    match visibility {
        AstFunctionVisibility::Private => FunctionVisibility::Private,
        AstFunctionVisibility::Public => FunctionVisibility::Public,
        AstFunctionVisibility::Export => FunctionVisibility::DatapackExport,
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
        IMMUTABLE_ASSIGNMENT, INTEGER_OUT_OF_RANGE, INVALID_ENTITY_TAG, INVALID_EXECUTOR_CAPTURE,
        INVALID_MESSAGE_LITERAL, INVALID_MINECRAFT_METHOD_RECEIVER, INVALID_UNSAFE_COMMAND,
        MESSAGE_LITERAL_REQUIRED, MISSING_RETURN, RESERVED_COMPILER_NAME, RETURN_IN_RUN_SCOPE,
        RETURN_VALUE_FORBIDDEN, RETURN_VALUE_REQUIRED, SCOPED_CAPABILITY_VALUE, TRUNCATED,
        TYPE_MISMATCH, UNINITIALIZED_READ, UNKNOWN_MEMBER, UNKNOWN_NAME, UNRESOLVED_MEMBER,
        UNSUPPORTED_RUN_SCALAR_CAPTURE, VOID_VALUE, check,
    };
    use crate::frontend::FrontendLimits;
    use crate::frontend::hir::{FunctionResult, HirStatementKind, SourceFunctionId, ValueType};
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
    fn unsafe_command_decoding_is_validated_before_hir_identity_allocation() {
        let (sources, _, output) =
            check_text(r#"fn raw() { unsafe minecraft("say \"quoted\" \\ path"); }"#);
        assert_eq!(output.diagnostics(), None);
        let checked = output.checked().unwrap();
        assert_eq!(checked.external_operation_count(), 1);
        let dump = checked.dump(&sources);
        assert!(dump.contains("external @0 unsafe.minecraft"));

        for invalid in [
            r#"fn raw() { unsafe minecraft(""); }"#,
            r#"fn raw() { unsafe minecraft(" say hi"); }"#,
            r#"fn raw() { unsafe minecraft("say hi "); }"#,
            r#"fn raw() { unsafe minecraft("/say hi"); }"#,
            r#"fn raw() { unsafe minecraft("say a\nb"); }"#,
            r#"fn raw() { unsafe minecraft("say a\rb"); }"#,
            r#"fn raw() { unsafe minecraft("say hi\\"); }"#,
        ] {
            let (_, _, output) = check_text(invalid);
            assert_eq!(codes(&output), [INVALID_UNSAFE_COMMAND], "{invalid}");
            assert!(output.checked().is_none());
        }
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
    fn checks_typed_static_run_query_and_non_escaping_executor_capture() {
        let source = r#"fn scoped() {
    run.as(mc.entities(ArmorStand).with_tag("stage7").limit(1)) |speaker| {
        unsafe minecraft("say hello");
    }
}"#;
        let (sources, _, output) = check_text(source);
        assert_eq!(output.diagnostics(), None);
        let checked = output.checked().unwrap();
        assert_eq!(checked.run_scope_count(), 1);
        let dump = checked.dump(&sources);
        assert!(dump.contains("as entities(ArmorStand).with_tag(\"stage7\").limit(1)"));
        assert!(dump.contains("capture speaker: Executor<ArmorStand>"));
    }

    #[test]
    fn typed_say_resolves_only_through_the_exact_current_executor_proof() {
        let source = r#"fn scoped() {
            run.as(mc.entities(ArmorStand).limit(1)) |speaker| {
                run { speaker.say("hello"); }
            }
        }"#;
        let (sources, _, output) = check_text(source);
        assert_eq!(output.diagnostics(), None);
        let checked = output.checked().unwrap();
        assert_eq!(checked.external_operation_count(), 1);
        let dump = checked.dump(&sources);
        assert!(dump.contains("minecraft.Say receiver=Executor<ArmorStand>"));
        assert!(dump.contains("message=\"hello\""));

        let (_, _, stale) = check_text(
            r#"fn bad() {
                run.as(mc.entities(ArmorStand).limit(1)) |outer| {
                    run.as(mc.entities(ArmorStand).limit(1)) |inner| {
                        outer.say("hello");
                    }
                }
            }"#,
        );
        assert_eq!(codes(&stale), [SCOPED_CAPABILITY_VALUE]);
    }

    #[test]
    fn typed_say_validates_its_closed_literal_signature_and_source_void_result() {
        for source in [
            r#"fn bad() { run.as(mc.entities(ArmorStand).limit(1)) |s| { s.say(""); } }"#,
            r#"fn bad() { run.as(mc.entities(ArmorStand).limit(1)) |s| { s.say("line\nnext"); } }"#,
            r#"fn bad() { run.as(mc.entities(ArmorStand).limit(1)) |s| { s.say("hello @s"); } }"#,
        ] {
            let (_, _, output) = check_text(source);
            assert_eq!(codes(&output), [INVALID_MESSAGE_LITERAL], "{source}");
        }

        let (_, _, non_literal) =
            check_text("fn bad() { run.as(mc.entities(ArmorStand).limit(1)) |s| { s.say(1); } }");
        assert_eq!(codes(&non_literal), [MESSAGE_LITERAL_REQUIRED]);

        let (_, _, arity) =
            check_text("fn bad() { run.as(mc.entities(ArmorStand).limit(1)) |s| { s.say(); } }");
        assert_eq!(codes(&arity), [ARGUMENT_COUNT]);

        let (_, _, as_value) = check_text(
            r#"fn bad() { run.as(mc.entities(ArmorStand).limit(1)) |s| {
                var value: Int32 = s.say("hello");
            } }"#,
        );
        assert_eq!(codes(&as_value), [VOID_VALUE]);

        let (_, _, unknown) = check_text(
            r#"fn bad() { run.as(mc.entities(ArmorStand).limit(1)) |s| { s.nope("hello"); } }"#,
        );
        assert_eq!(codes(&unknown), [UNKNOWN_MEMBER]);
    }

    #[test]
    fn typed_say_rejects_an_ordinary_value_with_the_executor_capture_contract() {
        let source = r#"fn bad() {
            var value: Int32 = 1;
            value.say("hello");
        }"#;
        let (sources, _, output) = check_text(source);
        assert_eq!(codes(&output), [INVALID_MINECRAFT_METHOD_RECEIVER]);

        let diagnostic = &output.diagnostics().unwrap().findings()[0];
        assert_eq!(
            spelling(&sources, diagnostic_span(&sources, &output, 0)),
            "value"
        );
        assert_eq!(
            diagnostic.message(),
            "Minecraft method `say` requires the current Executor<T: CommandExecutor> capture"
        );
        assert_eq!(diagnostic.supporting_labels().len(), 1);
        let declaration = sources
            .resolve_origin_span(diagnostic.supporting_labels()[0].origin())
            .unwrap();
        assert_eq!(spelling(&sources, declaration), "value");
        assert!(declaration.start() < diagnostic_span(&sources, &output, 0).start());

        let (_, _, unknown) =
            check_text("fn bad() { var value: Int32 = 1; value.unregistered_method(); }");
        assert_eq!(codes(&unknown), [UNRESOLVED_MEMBER]);
    }

    #[test]
    fn repeated_run_as_is_ordered_and_zero_modifier_nested_run_inherits_context() {
        let source = r#"fn scoped() {
            run.as(mc.entities(ArmorStand).limit(1))
                .as(mc.entities(ArmorStand).with_tag("second").limit(1)) |outer| {
                run |inner| {}
            }
        }"#;
        let (_, _, output) = check_text(source);
        assert_eq!(output.diagnostics(), None);
        let checked = output.checked().unwrap();
        let HirStatementKind::Run(outer) =
            &checked.functions().first().unwrap().body.statements[0].kind
        else {
            unreachable!();
        };
        assert_eq!(outer.modifiers.len(), 2);
        let outer_proof = outer.capture.as_ref().unwrap().proof;
        assert_eq!(outer_proof.modifier_index, 1);

        let HirStatementKind::Run(inner) = &outer.body.statements[0].kind else {
            unreachable!();
        };
        assert!(inner.modifiers.is_empty());
        assert_eq!(inner.resulting_context, outer.resulting_context);
        assert_eq!(inner.capture.as_ref().unwrap().proof, outer_proof);
    }

    #[test]
    fn compiler_minecraft_namespace_and_nominal_kinds_cannot_be_shadowed() {
        for source in [
            "fn mc() {}",
            "fn ArmorStand() {}",
            "fn bad(mc: Int32) {}",
            "fn bad() { var ArmorStand: Int32 = 0; }",
            r#"const mc = import("anything"); fn valid() {}"#,
            "fn bad() { run.as(mc.entities(ArmorStand).limit(1)) |mc| {} }",
        ] {
            let (_, _, output) = check_text(source);
            assert_eq!(codes(&output), [RESERVED_COMPILER_NAME], "{source}");
            assert!(output.checked().is_none());
        }
    }

    #[test]
    fn run_scope_diagnostics_keep_context_capabilities_and_scalars_lexical() {
        let (_, _, no_executor) = check_text("fn bad() { run |speaker| {} }");
        assert_eq!(codes(&no_executor), [INVALID_EXECUTOR_CAPTURE]);

        let (_, _, bad_tag) = check_text(
            r#"fn bad() { run.as(mc.entities(ArmorStand).with_tag("two words").limit(1)) {} }"#,
        );
        assert_eq!(codes(&bad_tag), [INVALID_ENTITY_TAG]);

        let (_, _, returning) =
            check_text("fn bad() { run.as(mc.entities(ArmorStand).limit(1)) { return; } }");
        assert_eq!(codes(&returning), [RETURN_IN_RUN_SCOPE]);

        let (_, _, implicit_scalar) = check_text(
            "fn bad(value: Int32) { run.as(mc.entities(ArmorStand).limit(1)) { var copy: Int32 = value; } }",
        );
        assert_eq!(codes(&implicit_scalar), [UNSUPPORTED_RUN_SCALAR_CAPTURE]);
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
