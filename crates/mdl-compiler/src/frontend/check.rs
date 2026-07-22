//! Signature collection, semantic checking, and structured flow analysis.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;
use std::rc::Rc;

use super::FrontendLimits;
use super::ast::{
    AstAssignment, AstBindingKind, AstBlock, AstCall, AstCallStatement, AstComparisonOp,
    AstCoordinateSigil, AstDeclaration, AstDestructureTargetKind, AstDestructuringStatement,
    AstEventHandler, AstEventHandlerArgumentValue, AstExpression, AstExpressionKind, AstFunction,
    AstFunctionVisibility, AstIfStatement, AstInferredStructEntries, AstInferredStructLiteral,
    AstModule, AstName, AstResultTypeKind, AstReturnStatement, AstRunModifier, AstRunStatement,
    AstStatement, AstStructLiteral, AstSwitchExpression, AstSwitchLabel, AstSwitchPattern,
    AstSwitchPatternKind, AstSwitchStatement, AstValueTypeKind, AstWhileStatement,
    AstWrappingArithmeticOp,
};
use super::context::{apply_run_modifiers, function_entry_context};
use super::entity_schema::{SchemaNode, block_root_schema, root_schema};
use super::hir::{
    CheckedFrontendOutput, FunctionResult, FunctionVisibility, HirAnonymousStruct,
    HirAnonymousStructField, HirAnonymousStructKind, HirBinding, HirBindingKind, HirBlock, HirCall,
    HirComparisonOp, HirContextStep, HirCriterion, HirDestructureTarget, HirDestructureTargetRole,
    HirEntityNbtReceiver, HirEntityPathSegment, HirEntityQuery, HirEntityQueryStep, HirEnum,
    HirEnumVariant, HirEventHandler, HirExecutionContext, HirExecutorCapture, HirExpression,
    HirExpressionKind, HirExternalOp, HirExternalSemantic, HirFunction, HirIf, HirIfArm,
    HirListI32Op, HirMinecraftOperationAttributes, HirModule, HirModuleInfo, HirRun,
    HirRunModifier, HirStatement, HirStatementKind, HirStringOp, HirStruct, HirStructField,
    HirStructFieldValue, HirSwitchExpression, HirSwitchExpressionArm, HirSwitchLabel,
    HirSwitchPattern, HirSwitchPatternKind, HirSwitchStatement, HirSwitchStatementArm,
    HirVerificationError, HirWhile, HirWrappingArithmeticOp, LocalId, SourceAnonymousStructId,
    SourceEnumId, SourceExternalOpId, SourceFunctionId, SourceModuleId, SourceRunId,
    SourceStructId, SourceVariantId, ValueType, verify,
};
use super::input::{ModuleDependency, ModuleKey};
use crate::diagnostic::{Diagnostic, DiagnosticLabel, Diagnostics};
use crate::ir::command_line::validate_command_line_shape;
use crate::ir::semantic::{
    Axes, BlockEntityKind, BlockPosition, ContextFact, DimensionKey, EntityAnchor,
    EntityCapability, EntityKind, EntityTag, ExecutorType, FiniteDecimal, LocalPosition,
    MAX_PACKAGE_RUN_MODIFIERS, MAX_RUN_MODIFIERS_PER_SCOPE, MessageLiteral, PositionSpec,
    RotationAxis, RotationSpec, SemanticType, SourceReceiverRule, StaticEntityQuery, WorldAxis,
    WorldPosition, minecraft_descriptor, minecraft_source_methods, resolve_minecraft_method,
};
use crate::source::{Origin, OriginError, SourceContext, SourceError, Span};

const DUPLICATE_FUNCTION: &str = "frontend.check.duplicate-function";
const DUPLICATE_STRUCT: &str = "frontend.check.duplicate-struct";
const DUPLICATE_TYPE: &str = "frontend.check.duplicate-type";
const EMPTY_ENUM: &str = "frontend.check.empty-enum";
const DUPLICATE_ENUM_VARIANT: &str = "frontend.check.duplicate-enum-variant";
const UNKNOWN_ENUM_VARIANT: &str = "frontend.check.unknown-enum-variant";
const ENUM_CONTEXT_REQUIRED: &str = "frontend.check.enum-context-required";
const INVALID_SWITCH_PATTERN: &str = "frontend.check.invalid-switch-pattern";
const OVERLAPPING_SWITCH_PATTERN: &str = "frontend.check.overlapping-switch-pattern";
const NON_EXHAUSTIVE_SWITCH: &str = "frontend.check.non-exhaustive-switch";
const INVALID_SWITCH_ELSE: &str = "frontend.check.invalid-switch-else";
const ENUM_EXPORT_ABI: &str = "frontend.check.enum-export-abi";
const DUPLICATE_STRUCT_FIELD: &str = "frontend.check.duplicate-struct-field";
const UNKNOWN_TYPE: &str = "frontend.check.unknown-type";
const UNKNOWN_STRUCT_FIELD: &str = "frontend.check.unknown-struct-field";
const DUPLICATE_STRUCT_INITIALIZER: &str = "frontend.check.duplicate-struct-initializer";
const MISSING_STRUCT_FIELD: &str = "frontend.check.missing-struct-field";
const RECURSIVE_STRUCT: &str = "frontend.check.recursive-struct";
const STRUCT_FIELD_LIMIT: &str = "frontend.check.struct-field-limit";
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
const LOOP_CONTROL_OUTSIDE_LOOP: &str = "frontend.check.loop-control-outside-loop";
const LITERAL_CONTEXT_REQUIRED: &str = "frontend.check.literal-context-required";
const UNSUPPORTED_RUN_SCALAR_CAPTURE: &str = "frontend.check.unsupported-run-scalar-capture";
const DIRTY_AST: &str = "frontend.check.dirty-ast";
const TRUNCATED: &str = "frontend.check.truncated";
const UNKNOWN_EVENT_TRIGGER: &str = "frontend.check.unknown-event-trigger";
const UNKNOWN_EVENT_ARGUMENT: &str = "frontend.check.unknown-event-argument";
const DUPLICATE_EVENT_ARGUMENT: &str = "frontend.check.duplicate-event-argument";
const MISSING_EVENT_ARGUMENT: &str = "frontend.check.missing-event-argument";
const INVALID_EVENT_ITEM: &str = "frontend.check.invalid-event-item";

/// Closed event-trigger vocabulary (PS-15 Slice 1: one variant). Growing this
/// to a second trigger is "add a variant", not a redesign of the argument-
/// contract dispatch below.
#[derive(Clone, Copy)]
enum EventTrigger {
    InventoryChanged,
}

impl EventTrigger {
    fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "inventory_changed" => Some(Self::InventoryChanged),
            _ => None,
        }
    }
}

/// Validates the target-independent `namespace:path` shape vanilla resource
/// locations use (e.g. item IDs in advancement JSON `conditions`). Mirrors
/// `ir::minecraft::names`'s `Namespace`/`ResourcePath` character classes
/// without depending on that target-facing type: Core (and this checker
/// validation that feeds it) must never depend on `ir::minecraft`.
fn is_valid_item_resource_id(text: &str) -> bool {
    fn is_valid_namespace_char(c: char) -> bool {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-')
    }
    fn is_valid_path_char(c: char) -> bool {
        is_valid_namespace_char(c) || c == '/'
    }
    let Some((namespace, path)) = text.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !path.is_empty()
        && namespace.chars().all(is_valid_namespace_char)
        && path.chars().all(is_valid_path_char)
}

fn is_reserved_compiler_name(name: &str) -> bool {
    matches!(name, "mc" | "List")
        || EntityKind::from_source_name(name).is_some()
        || BlockEntityKind::from_source_name(name).is_some()
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
    Struct,
    Enum,
    Variant,
    Function,
    ExternalOperation,
    RunScope,
    Local,
}

impl fmt::Display for CheckedEntityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Module => formatter.write_str("module"),
            Self::Struct => formatter.write_str("struct"),
            Self::Enum => formatter.write_str("enum"),
            Self::Variant => formatter.write_str("variant"),
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
#[allow(
    clippy::too_many_lines,
    reason = "package checking materializes every closed HIR inventory in source order"
)]
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

    let mut hir_structs = Vec::with_capacity(signatures.structs.len());
    for struct_ in &signatures.structs {
        let mut fields = Vec::with_capacity(struct_.fields.len());
        for field in &struct_.fields {
            fields.push(HirStructField {
                name_origin: sources.add_origin(Origin::Source(field.name_span))?,
                ty: field.ty,
                type_origin: sources.add_origin(Origin::Source(field.type_span))?,
                origin: sources.add_origin(Origin::Source(field.span))?,
            });
        }
        hir_structs.push(HirStruct {
            id: struct_.id,
            module: struct_.module,
            name_origin: sources.add_origin(Origin::Source(struct_.name_span))?,
            fields: fields.into_boxed_slice(),
            origin: sources.add_origin(Origin::Source(struct_.span))?,
        });
    }

    let mut hir_enums = Vec::with_capacity(signatures.enums.len());
    for enum_ in &signatures.enums {
        let variants = enum_
            .variants
            .iter()
            .map(|variant| {
                Ok(HirEnumVariant {
                    id: variant.id,
                    name_origin: sources.add_origin(Origin::Source(variant.name_span))?,
                    origin: sources.add_origin(Origin::Source(variant.span))?,
                })
            })
            .collect::<Result<Vec<_>, OriginError>>()?;
        hir_enums.push(HirEnum {
            id: enum_.id,
            module: enum_.module,
            name_origin: sources.add_origin(Origin::Source(enum_.name_span))?,
            variants: variants.into_boxed_slice(),
            origin: sources.add_origin(Origin::Source(enum_.span))?,
        });
    }

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

    let mut event_handlers = Vec::with_capacity(
        modules
            .iter()
            .map(|module| module.ast.event_handlers.len())
            .sum(),
    );
    for module in modules {
        for handler_ast in &module.ast.event_handlers {
            let id = SourceFunctionId::from_index(functions.len()).ok_or(
                CheckError::IdentitySpaceExhausted(CheckedEntityKind::Function),
            )?;
            let signature = FunctionSignature {
                id,
                module: module.id,
                ast_index: 0,
                name: Box::from(""),
                name_span: handler_ast.trigger.span,
                visibility: FunctionVisibility::Private,
                parameters: Box::new([]),
                result: FunctionResult::Void,
                result_span: None,
            };
            let checker = BodyChecker::new(
                sources,
                &mut diagnostics,
                &signatures,
                &signature,
                &mut external_ops,
                &mut next_run_scope,
                &mut next_run_modifier,
            );
            let (function, criterion) = checker.check_event_handler(handler_ast)?;
            functions.push(function);
            if let Some(criterion) = criterion {
                event_handlers.push(HirEventHandler {
                    reward: id,
                    criterion,
                    origin: sources.add_origin(Origin::Source(handler_ast.span))?,
                });
            }
        }
    }

    let hir_anonymous_structs = signatures
        .anonymous
        .borrow()
        .entries
        .iter()
        .map(|anonymous| {
            let origin = sources.add_origin(Origin::Source(anonymous.span))?;
            let kind = match &anonymous.key {
                AnonymousTypeKey::Named(fields) => HirAnonymousStructKind::Named(
                    fields
                        .iter()
                        .map(|(name, ty)| {
                            Ok(HirAnonymousStructField {
                                name: name.clone(),
                                ty: *ty,
                                origin,
                            })
                        })
                        .collect::<Result<Vec<_>, OriginError>>()?
                        .into_boxed_slice(),
                ),
                AnonymousTypeKey::Positional(fields) => {
                    HirAnonymousStructKind::Positional(fields.clone())
                }
            };
            Ok(HirAnonymousStruct {
                id: anonymous.id,
                kind,
                origin,
            })
        })
        .collect::<Result<Vec<_>, OriginError>>()?;

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
        structs: hir_structs.into_boxed_slice(),
        enums: hir_enums.into_boxed_slice(),
        anonymous_structs: hir_anonymous_structs.into_boxed_slice(),
        external_ops,
        run_scope_count: next_run_scope,
        functions,
        behaviors,
        event_handlers: event_handlers.into_boxed_slice(),
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

struct StructSignature {
    id: SourceStructId,
    module: SourceModuleId,
    ast_index: usize,
    name_span: Span,
    fields: Box<[StructFieldSignature]>,
    span: Span,
}

struct StructFieldSignature {
    name: Box<str>,
    name_span: Span,
    ty: ValueType,
    type_span: Span,
    span: Span,
}

struct EnumSignature {
    id: SourceEnumId,
    module: SourceModuleId,
    name_span: Span,
    variants: Box<[EnumVariantSignature]>,
    span: Span,
}

struct EnumVariantSignature {
    id: SourceVariantId,
    name: Box<str>,
    name_span: Span,
    span: Span,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum AnonymousTypeKey {
    Named(Box<[(Box<str>, ValueType)]>),
    Positional(Box<[ValueType]>),
}

struct AnonymousSignature {
    id: SourceAnonymousStructId,
    key: AnonymousTypeKey,
    span: Span,
}

#[derive(Default)]
struct AnonymousInterner {
    entries: Vec<AnonymousSignature>,
    by_key: HashMap<AnonymousTypeKey, SourceAnonymousStructId>,
}

#[derive(Clone, Copy)]
enum TypeLookup {
    Struct { id: SourceStructId, span: Span },
    Enum { id: SourceEnumId, span: Span },
}

type TypesByModule = Vec<HashMap<Box<str>, TypeLookup>>;

#[derive(Clone, Copy)]
struct ParameterSignature {
    ty: ValueType,
    type_span: Span,
}

struct SignatureIndex {
    structs: Vec<StructSignature>,
    enums: Vec<EnumSignature>,
    anonymous: RefCell<AnonymousInterner>,
    types_by_module: TypesByModule,
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
    let (mut structs, mut types_by_module) = collect_struct_headers(sources, modules, diagnostics)?;
    let anonymous = RefCell::new(AnonymousInterner::default());
    let enums = collect_enum_headers(sources, modules, &mut types_by_module, diagnostics)?;
    resolve_struct_fields(
        sources,
        modules,
        &types_by_module,
        &anonymous,
        &mut structs,
        diagnostics,
    )?;
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
            &types_by_module,
            &anonymous,
            diagnostics,
        )?;
        namespaces.push(imports.namespaces);
        by_module.push(functions_by_name);
    }

    for function in &functions {
        if function.visibility != FunctionVisibility::DatapackExport {
            continue;
        }
        for parameter in &function.parameters {
            if type_contains_enum(parameter.ty, &structs, &mut vec![]) {
                diagnostics.push(
                    PendingDiagnostic::new(
                        ENUM_EXPORT_ABI,
                        "datapack exports cannot expose enum-containing parameter types",
                        parameter.type_span,
                    )
                    .primary("unvalidated closed enum ABI"),
                );
            }
        }
        if let FunctionResult::Value(ty) = function.result {
            if type_contains_enum(ty, &structs, &mut vec![]) {
                diagnostics.push(
                    PendingDiagnostic::new(
                        ENUM_EXPORT_ABI,
                        "datapack exports cannot expose enum-containing result types",
                        function.result_span.unwrap_or(function.name_span),
                    )
                    .primary("unvalidated closed enum ABI"),
                );
            }
        }
    }

    Ok(SignatureIndex {
        structs,
        enums,
        anonymous,
        types_by_module,
        functions,
        by_module,
        namespaces,
    })
}

fn type_contains_enum(
    ty: ValueType,
    structs: &[StructSignature],
    visiting: &mut Vec<SourceStructId>,
) -> bool {
    match ty {
        ValueType::Enum(_) => true,
        ValueType::Struct(id) => {
            if visiting.contains(&id) {
                return false;
            }
            visiting.push(id);
            let result = id
                .as_usize()
                .and_then(|index| structs.get(index))
                .is_some_and(|struct_| {
                    struct_
                        .fields
                        .iter()
                        .any(|field| type_contains_enum(field.ty, structs, visiting))
                });
            visiting.pop();
            result
        }
        _ => false,
    }
}

fn collect_struct_headers(
    sources: &SourceContext,
    modules: &[PackageAstModule<'_>],
    diagnostics: &mut DiagnosticSink,
) -> Result<(Vec<StructSignature>, TypesByModule), CheckError> {
    let mut structs = Vec::new();
    let mut by_module = Vec::with_capacity(modules.len());
    for module in modules {
        let mut names = HashMap::new();
        let mut first_spans = HashMap::<Box<str>, Span>::new();
        for (ast_index, struct_) in module.ast.structs.iter().enumerate() {
            let id = SourceStructId::from_index(structs.len()).ok_or(
                CheckError::IdentitySpaceExhausted(CheckedEntityKind::Struct),
            )?;
            let name: Box<str> = sources.files().slice(struct_.name.span)?.into();
            if let Some(original) = first_spans.get(name.as_ref()).copied() {
                diagnostics.push(
                    PendingDiagnostic::new(
                        DUPLICATE_STRUCT,
                        format!("struct `{name}` is declared more than once"),
                        struct_.name.span,
                    )
                    .primary("duplicate struct declaration")
                    .support(original, "first declaration is here"),
                );
            } else {
                first_spans.insert(name.clone(), struct_.name.span);
                names.insert(
                    name.clone(),
                    TypeLookup::Struct {
                        id,
                        span: struct_.name.span,
                    },
                );
            }
            structs.push(StructSignature {
                id,
                module: module.id,
                ast_index,
                name_span: struct_.name.span,
                fields: Box::new([]),
                span: struct_.span,
            });
        }
        by_module.push(names);
    }
    Ok((structs, by_module))
}

fn collect_enum_headers(
    sources: &SourceContext,
    modules: &[PackageAstModule<'_>],
    by_module: &mut [HashMap<Box<str>, TypeLookup>],
    diagnostics: &mut DiagnosticSink,
) -> Result<Vec<EnumSignature>, CheckError> {
    let mut enums = vec![];
    for module in modules {
        let module_index = module
            .id
            .as_usize()
            .ok_or(CheckError::IdentitySpaceExhausted(
                CheckedEntityKind::Module,
            ))?;
        for enum_ in &module.ast.enums {
            let id = SourceEnumId::from_index(enums.len())
                .ok_or(CheckError::IdentitySpaceExhausted(CheckedEntityKind::Enum))?;
            let name: Box<str> = sources.files().slice(enum_.name.span)?.into();
            if is_reserved_compiler_name(&name) {
                diagnostics.push(reserved_compiler_name_diagnostic(&name, enum_.name.span));
            }
            if let Some(previous) = by_module[module_index].get(name.as_ref()) {
                let previous_span = match previous {
                    TypeLookup::Struct { span, .. } | TypeLookup::Enum { span, .. } => *span,
                };
                diagnostics.push(
                    PendingDiagnostic::new(
                        DUPLICATE_TYPE,
                        format!("type `{name}` is declared more than once"),
                        enum_.name.span,
                    )
                    .primary("duplicate nominal type declaration")
                    .support(previous_span, "first declaration is here"),
                );
            } else {
                by_module[module_index].insert(
                    name.clone(),
                    TypeLookup::Enum {
                        id,
                        span: enum_.name.span,
                    },
                );
            }
            if enum_.variants.is_empty() {
                diagnostics.push(
                    PendingDiagnostic::new(
                        EMPTY_ENUM,
                        "enum must declare at least one variant",
                        enum_.span,
                    )
                    .primary("empty enum"),
                );
            }
            let mut first = HashMap::<Box<str>, Span>::new();
            let mut variants = vec![];
            for (index, variant) in enum_.variants.iter().enumerate() {
                let variant_id = SourceVariantId::from_index(index).ok_or(
                    CheckError::IdentitySpaceExhausted(CheckedEntityKind::Variant),
                )?;
                let variant_name: Box<str> = sources.files().slice(variant.name.span)?.into();
                if is_reserved_compiler_name(&variant_name) {
                    diagnostics.push(reserved_compiler_name_diagnostic(
                        &variant_name,
                        variant.name.span,
                    ));
                }
                if let Some(original) = first.insert(variant_name.clone(), variant.name.span) {
                    diagnostics.push(
                        PendingDiagnostic::new(
                            DUPLICATE_ENUM_VARIANT,
                            format!("variant `{variant_name}` is declared more than once"),
                            variant.name.span,
                        )
                        .primary("duplicate enum variant")
                        .support(original, "first variant is here"),
                    );
                }
                variants.push(EnumVariantSignature {
                    id: variant_id,
                    name: variant_name,
                    name_span: variant.name.span,
                    span: variant.span,
                });
            }
            enums.push(EnumSignature {
                id,
                module: module.id,
                name_span: enum_.name.span,
                variants: variants.into_boxed_slice(),
                span: enum_.span,
            });
        }
    }
    Ok(enums)
}

fn resolve_struct_fields(
    sources: &SourceContext,
    modules: &[PackageAstModule<'_>],
    by_module: &[HashMap<Box<str>, TypeLookup>],
    anonymous: &RefCell<AnonymousInterner>,
    structs: &mut [StructSignature],
    diagnostics: &mut DiagnosticSink,
) -> Result<(), CheckError> {
    for struct_ in structs.iter_mut() {
        let module_index = struct_
            .module
            .as_usize()
            .ok_or(CheckError::IdentitySpaceExhausted(
                CheckedEntityKind::Module,
            ))?;
        let ast = &modules[module_index].ast.structs[struct_.ast_index];
        let mut fields = Vec::with_capacity(ast.fields.len());
        if ast.fields.len() > 64 {
            diagnostics.push(
                PendingDiagnostic::new(
                    STRUCT_FIELD_LIMIT,
                    format!(
                        "struct has {} fields; Phase 1 permits at most 64",
                        ast.fields.len()
                    ),
                    ast.span,
                )
                .primary("split this value into smaller nominal structs"),
            );
        }
        let mut first_spans = HashMap::<Box<str>, Span>::new();
        for field in &ast.fields {
            let name: Box<str> = sources.files().slice(field.name.span)?.into();
            if let Some(original) = first_spans.insert(name.clone(), field.name.span) {
                diagnostics.push(
                    PendingDiagnostic::new(
                        DUPLICATE_STRUCT_FIELD,
                        format!("field `{name}` is declared more than once"),
                        field.name.span,
                    )
                    .primary("duplicate field")
                    .support(original, "first field is here"),
                );
            }
            let ty = resolve_value_type(
                sources,
                struct_.module,
                field.ty.kind.clone(),
                by_module,
                anonymous,
                diagnostics,
            )
            .unwrap_or(ValueType::Int32);
            fields.push(StructFieldSignature {
                name,
                name_span: field.name.span,
                ty,
                type_span: field.ty.span,
                span: field.span,
            });
        }
        struct_.fields = fields.into_boxed_slice();
    }
    reject_recursive_structs(structs, diagnostics);
    Ok(())
}

fn reject_recursive_structs(structs: &[StructSignature], diagnostics: &mut DiagnosticSink) {
    fn visit(
        index: usize,
        structs: &[StructSignature],
        state: &mut [u8],
        diagnostics: &mut DiagnosticSink,
    ) {
        state[index] = 1;
        for field in &structs[index].fields {
            let ValueType::Struct(target) = field.ty else {
                continue;
            };
            let Some(target) = target.as_usize().filter(|target| *target < structs.len()) else {
                continue;
            };
            if state[target] == 1 {
                diagnostics.push(
                    PendingDiagnostic::new(
                        RECURSIVE_STRUCT,
                        "struct values cannot contain themselves recursively",
                        field.type_span,
                    )
                    .primary("this field closes an infinitely sized value cycle")
                    .support(structs[target].name_span, "cycle reaches this struct"),
                );
            } else if state[target] == 0 {
                visit(target, structs, state, diagnostics);
            }
        }
        state[index] = 2;
    }

    let mut state = vec![0u8; structs.len()];
    for index in 0..structs.len() {
        if state[index] == 0 {
            visit(index, structs, &mut state, diagnostics);
        }
    }
}

fn resolve_value_type(
    sources: &SourceContext,
    module: SourceModuleId,
    kind: AstValueTypeKind,
    by_module: &[HashMap<Box<str>, TypeLookup>],
    anonymous: &RefCell<AnonymousInterner>,
    diagnostics: &mut DiagnosticSink,
) -> Option<ValueType> {
    match kind {
        AstValueTypeKind::Bool => Some(ValueType::Bool),
        AstValueTypeKind::Int32 => Some(ValueType::Int32),
        AstValueTypeKind::ListI32 => Some(ValueType::ListI32),
        AstValueTypeKind::String => Some(ValueType::String),
        AstValueTypeKind::Named(name) => {
            let spelling = sources.files().slice(name.span).ok()?;
            let resolved = module
                .as_usize()
                .and_then(|index| by_module.get(index))
                .and_then(|types| types.get(spelling))
                .copied();
            if resolved.is_none() {
                diagnostics.push(
                    PendingDiagnostic::new(
                        UNKNOWN_TYPE,
                        format!("unknown type `{spelling}`"),
                        name.span,
                    )
                    .primary("no nominal type with this name exists in the module"),
                );
            }
            resolved.map(|resolved| match resolved {
                TypeLookup::Struct { id, .. } => ValueType::Struct(id),
                TypeLookup::Enum { id, .. } => ValueType::Enum(id),
            })
        }
        AstValueTypeKind::Anonymous(anonymous_ast) => {
            use super::ast::AstAnonymousStructTypeKind;
            let key = match anonymous_ast.kind {
                AstAnonymousStructTypeKind::Named(fields) => {
                    let mut resolved = Vec::with_capacity(fields.len());
                    let mut names = HashMap::<Box<str>, Span>::new();
                    for field in fields {
                        let name: Box<str> = sources.files().slice(field.name.span).ok()?.into();
                        if let Some(original) = names.insert(name.clone(), field.name.span) {
                            diagnostics.push(
                                PendingDiagnostic::new(
                                    DUPLICATE_STRUCT_FIELD,
                                    format!(
                                        "anonymous struct field `{name}` is declared more than once"
                                    ),
                                    field.name.span,
                                )
                                .support(original, "first field is here"),
                            );
                        }
                        let ty = resolve_value_type(
                            sources,
                            module,
                            field.ty.kind,
                            by_module,
                            anonymous,
                            diagnostics,
                        )?;
                        resolved.push((name, ty));
                    }
                    AnonymousTypeKey::Named(resolved.into_boxed_slice())
                }
                AstAnonymousStructTypeKind::Positional(components) => {
                    let mut resolved = Vec::with_capacity(components.len());
                    for component in components {
                        resolved.push(resolve_value_type(
                            sources,
                            module,
                            component.kind,
                            by_module,
                            anonymous,
                            diagnostics,
                        )?);
                    }
                    AnonymousTypeKey::Positional(resolved.into_boxed_slice())
                }
            };
            let mut interner = anonymous.borrow_mut();
            if let Some(id) = interner.by_key.get(&key).copied() {
                return Some(ValueType::AnonymousStruct(id));
            }
            let id = SourceAnonymousStructId::from_index(interner.entries.len())?;
            interner.entries.push(AnonymousSignature {
                id,
                key: key.clone(),
                span: anonymous_ast.span,
            });
            interner.by_key.insert(key, id);
            Some(ValueType::AnonymousStruct(id))
        }
    }
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
    types_by_module: &[HashMap<Box<str>, TypeLookup>],
    anonymous: &RefCell<AnonymousInterner>,
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
        } else if types_by_module
            .get(module.id.as_usize().unwrap_or(usize::MAX))
            .is_some_and(|types| types.contains_key(name.as_ref()))
        {
            diagnostics.push(
                PendingDiagnostic::new(
                    TOP_LEVEL_NAME_CONFLICT,
                    format!("function `{name}` conflicts with a nominal type"),
                    function.name.span,
                )
                .primary("top-level names share one namespace"),
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
        let mut parameters = Vec::with_capacity(function.parameters.len());
        for parameter in &function.parameters {
            let ty = resolve_value_type(
                sources,
                module.id,
                parameter.ty.kind.clone(),
                types_by_module,
                anonymous,
                diagnostics,
            )
            .unwrap_or(ValueType::Int32);
            parameters.push(ParameterSignature {
                ty,
                type_span: parameter.ty.span,
            });
        }
        let (result, result_span) = if let Some(result_ast) = &function.result {
            let result = match &result_ast.kind {
                AstResultTypeKind::Void => FunctionResult::Void,
                AstResultTypeKind::Value(kind) => FunctionResult::Value(
                    resolve_value_type(
                        sources,
                        module.id,
                        kind.clone(),
                        types_by_module,
                        anonymous,
                        diagnostics,
                    )
                    .unwrap_or(ValueType::Int32),
                ),
            };
            (result, Some(result_ast.span))
        } else {
            (FunctionResult::Void, None)
        };
        functions.push(FunctionSignature {
            id,
            module: module.id,
            ast_index,
            name,
            name_span: function.name.span,
            visibility: function_visibility(function.visibility),
            parameters: parameters.into_boxed_slice(),
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
    loop_depth: usize,
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
            loop_depth: 0,
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
            entry_capture: None,
            origin: function_origin,
        })
    }

    /// Checks one `on <trigger>(...) |binding| { ... }` declaration into a
    /// checked reward `HirFunction` (pushed into the same dense inventory as
    /// every ordinary function) plus its `HirCriterion`, or `None` for the
    /// criterion when the trigger/arguments were invalid (a diagnostic is
    /// always recorded in that case; `valid` on the returned function's
    /// checked block already reflects any body-level errors).
    fn check_event_handler(
        mut self,
        ast: &AstEventHandler,
    ) -> Result<(HirFunction, Option<HirCriterion>), CheckError> {
        let mut assigned = Assigned::default();

        let trigger_spelling = self.spelling(ast.trigger.span)?;
        let trigger = EventTrigger::from_source_name(&trigger_spelling);
        if trigger.is_none() {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNKNOWN_EVENT_TRIGGER,
                    format!("`{trigger_spelling}` is not a recognized event trigger"),
                    ast.trigger.span,
                )
                .primary("unrecognized trigger name"),
            );
        }
        let criterion = match trigger {
            Some(trigger) => self.check_event_handler_criterion(trigger, ast)?,
            None => None,
        };

        let mut valid = true;
        let proof_origin = self.origin(ast.binding.span)?;
        let proof = HirContextStep {
            run: SourceRunId::from_index(0).expect("run identity space always has index 0"),
            modifier_index: 0,
            origin: proof_origin,
        };
        let installed =
            self.install_executor_capture(ast.binding.span, EntityKind::Player, proof, &mut valid)?;
        self.execution_context = function_entry_context()
            .establish_executor(EntityKind::Player, proof)
            .establish_position(proof)
            .establish_rotation(proof)
            .establish_dimension(proof);

        let checked_body = self.check_block(&ast.body, &mut assigned)?;
        valid &= checked_body.valid;

        let name_origin = self.origin(ast.trigger.span)?;
        let function_origin = self.origin(ast.span)?;
        let function = HirFunction {
            id: self.signature.id,
            module: self.signature.module,
            visibility: FunctionVisibility::Private,
            visibility_origin: None,
            name_origin,
            parameter_count: 0,
            result: FunctionResult::Void,
            result_origin: None,
            bindings: self.bindings.into_boxed_slice(),
            body: checked_body.block,
            entry_capture: Some(installed.hir),
            origin: function_origin,
        };
        Ok((function, valid.then_some(criterion).flatten()))
    }

    fn check_event_handler_criterion(
        &mut self,
        trigger: EventTrigger,
        ast: &AstEventHandler,
    ) -> Result<Option<HirCriterion>, CheckError> {
        match trigger {
            EventTrigger::InventoryChanged => self.check_inventory_changed_criterion(ast),
        }
    }

    fn check_inventory_changed_criterion(
        &mut self,
        ast: &AstEventHandler,
    ) -> Result<Option<HirCriterion>, CheckError> {
        let mut valid = true;
        let mut items: Option<(Span, &[Span])> = None;
        for argument in &ast.arguments {
            let name = self.spelling(argument.name.span)?;
            if name.as_ref() != "items" {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        UNKNOWN_EVENT_ARGUMENT,
                        format!("`inventory_changed` has no trigger argument named `{name}`"),
                        argument.name.span,
                    )
                    .primary("unknown trigger argument"),
                );
                valid = false;
                continue;
            }
            if items.is_some() {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        DUPLICATE_EVENT_ARGUMENT,
                        "duplicate `.items` trigger argument",
                        argument.name.span,
                    )
                    .primary("`.items` was already supplied"),
                );
                valid = false;
                continue;
            }
            let AstEventHandlerArgumentValue::StringList(spans) = &argument.value;
            items = Some((argument.span, spans));
        }
        let Some((items_span, spans)) = items else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    MISSING_EVENT_ARGUMENT,
                    "`inventory_changed` requires a `.items` trigger argument",
                    ast.trigger.span,
                )
                .primary("no `.items` argument supplied"),
            );
            return Ok(None);
        };
        let mut decoded_items = Vec::with_capacity(spans.len());
        for &span in spans {
            let decoded = decode_string_literal(self.sources, span)?;
            let valid_item = decoded.as_deref().is_some_and(is_valid_item_resource_id);
            if !valid_item {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_EVENT_ITEM,
                        "expected a `namespace:path` item resource id",
                        span,
                    )
                    .primary("not a valid item resource id"),
                );
                valid = false;
                continue;
            }
            decoded_items.push(decoded.expect("checked above"));
        }
        if decoded_items.is_empty() {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    MISSING_EVENT_ARGUMENT,
                    "`.items` requires at least one item resource id",
                    items_span,
                )
                .primary("empty item list"),
            );
            valid = false;
        }
        if !valid {
            return Ok(None);
        }
        let items_origin = self.origin(items_span)?;
        Ok(Some(HirCriterion::InventoryChanged {
            items: decoded_items.into_boxed_slice(),
            items_origin,
        }))
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
            AstStatement::While(statement) => self.check_while(statement, assigned),
            AstStatement::Switch(statement) => self.check_switch_statement(statement, assigned),
            AstStatement::Break(span) => self.check_loop_control(*span, true),
            AstStatement::Continue(span) => self.check_loop_control(*span, false),
            AstStatement::Return(statement) => self.check_return(statement, assigned),
            AstStatement::Run(statement) => self.check_run(statement, assigned),
            AstStatement::UnsafeMinecraft(statement) => self.check_unsafe_minecraft(statement),
            AstStatement::Destructure(destructure) => {
                self.check_destructuring(destructure, assigned)
            }
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

    fn check_while(
        &mut self,
        statement: &AstWhileStatement,
        assigned: &mut Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let condition = self.check_expression(&statement.condition, assigned)?;
        let condition_is_bool = match condition.ty {
            Some(ValueType::Bool) => true,
            Some(actual) => {
                self.type_mismatch_without_support(
                    statement.condition.span,
                    actual,
                    ValueType::Bool,
                    "loop conditions must have type Bool",
                );
                false
            }
            None => false,
        };
        let checkpoint = assigned.checkpoint();
        self.loop_depth = self.loop_depth.saturating_add(1);
        let body = self.check_block(&statement.body, assigned);
        self.loop_depth = self.loop_depth.saturating_sub(1);
        assigned.rollback(checkpoint);
        let body = body?;

        let valid = condition_is_bool && condition.expression.is_some() && body.valid;
        let origin = self.origin(statement.span)?;
        let kind = match (valid, condition.expression) {
            (true, Some(condition)) => Some(HirStatementKind::While(HirWhile {
                condition,
                body: body.block,
                origin,
            })),
            _ => None,
        };
        if kind.is_none() {
            return Ok(CheckedStatement::invalid());
        }
        Ok(CheckedStatement::continuing(HirStatement {
            kind: kind.expect("validated loop kind exists"),
            origin,
        }))
    }

    fn check_loop_control(
        &mut self,
        span: Span,
        is_break: bool,
    ) -> Result<CheckedStatement, CheckError> {
        if self.loop_depth == 0 {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    LOOP_CONTROL_OUTSIDE_LOOP,
                    if is_break {
                        "`break` is only valid inside a loop"
                    } else {
                        "`continue` is only valid inside a loop"
                    },
                    span,
                )
                .primary("no enclosing loop is active here"),
            );
            return Ok(CheckedStatement::invalid());
        }
        let origin = self.origin(span)?;
        Ok(CheckedStatement {
            statement: Some(HirStatement {
                kind: if is_break {
                    HirStatementKind::Break
                } else {
                    HirStatementKind::Continue
                },
                origin,
            }),
            continues: false,
        })
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

    #[allow(
        clippy::too_many_lines,
        reason = "destructure checking keeps operand, target, and assignment rules atomic"
    )]
    fn check_destructuring(
        &mut self,
        statement: &AstDestructuringStatement,
        assigned: &mut Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let operand = self.check_expression(&statement.value, assigned)?;
        let Some(ValueType::AnonymousStruct(struct_id)) = operand.ty else {
            if operand.ty.is_some() {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        TYPE_MISMATCH,
                        "destructure operand must be a positional anonymous struct",
                        statement.value.span,
                    )
                    .primary("only positional anonymous struct values can be destructured"),
                );
            }
            return Ok(CheckedStatement::invalid());
        };
        let interner = self.signatures.anonymous.borrow();
        let anonymous = interner.entries.iter().find(|entry| entry.id == struct_id);
        let Some(anonymous) = anonymous else {
            return Ok(CheckedStatement::invalid());
        };
        let components: &[ValueType] = match &anonymous.key {
            AnonymousTypeKey::Positional(types) => types,
            AnonymousTypeKey::Named { .. } => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        TYPE_MISMATCH,
                        "destructure operand must be a positional anonymous struct",
                        statement.value.span,
                    )
                    .primary("named anonymous structs use field projection, not destructuring"),
                );
                return Ok(CheckedStatement::invalid());
            }
        };
        if statement.targets.len() != components.len() {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    ARGUMENT_COUNT,
                    format!(
                        "destructure requires {} targets but {} were provided",
                        components.len(),
                        statement.targets.len()
                    ),
                    statement.span,
                )
                .primary("destructure target count must match the operand's component count"),
            );
            return Ok(CheckedStatement::invalid());
        }

        let mut targets = Vec::with_capacity(statement.targets.len());
        let mut valid = operand.expression.is_some();
        let mut seen_names: HashMap<Rc<str>, Span> = HashMap::new();

        for (component_index, ast_target) in statement.targets.iter().enumerate() {
            let component_ty = components[component_index];
            let spelling: Rc<str> = Rc::from(self.spelling(ast_target.name.span)?);
            let name_origin = self.origin(ast_target.name.span)?;
            let target_origin = self.origin(ast_target.span)?;

            match ast_target.kind {
                AstDestructureTargetKind::Const => {
                    if let Some(original) =
                        seen_names.insert(Rc::clone(&spelling), ast_target.name.span)
                    {
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                DUPLICATE_BINDING,
                                format!("destructure target `{spelling}` appears more than once"),
                                ast_target.name.span,
                            )
                            .support(original, "first occurrence is here"),
                        );
                        valid = false;
                    }
                    let reserved = is_reserved_compiler_name(&spelling);
                    if reserved {
                        self.diagnostics.push(reserved_compiler_name_diagnostic(
                            &spelling,
                            ast_target.name.span,
                        ));
                        valid = false;
                    }
                    let local = self.allocate_binding(
                        HirBindingKind::Const,
                        component_ty,
                        ast_target.name.span,
                        ast_target.name.span,
                        ast_target.span,
                    )?;
                    self.install_active_binding(
                        Rc::clone(&spelling),
                        ActiveBinding::Unique(ScopeBinding {
                            local,
                            kind: HirBindingKind::Const,
                            ty: component_ty,
                            name_span: ast_target.name.span,
                            type_span: ast_target.name.span,
                        }),
                        local,
                    );
                    assigned.set(local, true);
                    targets.push(HirDestructureTarget {
                        role: HirDestructureTargetRole::Const,
                        local: Some(local),
                        component: u32::try_from(component_index).unwrap_or(u32::MAX),
                        ty: component_ty,
                        name_origin,
                        origin: target_origin,
                    });
                }
                AstDestructureTargetKind::Var => {
                    if let Some(original) =
                        seen_names.insert(Rc::clone(&spelling), ast_target.name.span)
                    {
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                DUPLICATE_BINDING,
                                format!("destructure target `{spelling}` appears more than once"),
                                ast_target.name.span,
                            )
                            .support(original, "first occurrence is here"),
                        );
                        valid = false;
                    }
                    let reserved = is_reserved_compiler_name(&spelling);
                    if reserved {
                        self.diagnostics.push(reserved_compiler_name_diagnostic(
                            &spelling,
                            ast_target.name.span,
                        ));
                        valid = false;
                    }
                    let local = self.allocate_binding(
                        HirBindingKind::Var,
                        component_ty,
                        ast_target.name.span,
                        ast_target.name.span,
                        ast_target.span,
                    )?;
                    self.install_active_binding(
                        Rc::clone(&spelling),
                        ActiveBinding::Unique(ScopeBinding {
                            local,
                            kind: HirBindingKind::Var,
                            ty: component_ty,
                            name_span: ast_target.name.span,
                            type_span: ast_target.name.span,
                        }),
                        local,
                    );
                    assigned.set(local, true);
                    targets.push(HirDestructureTarget {
                        role: HirDestructureTargetRole::Var,
                        local: Some(local),
                        component: u32::try_from(component_index).unwrap_or(u32::MAX),
                        ty: component_ty,
                        name_origin,
                        origin: target_origin,
                    });
                }
                AstDestructureTargetKind::Assign => {
                    let name: Rc<str> = Rc::from(self.spelling(ast_target.name.span)?);
                    if name.as_ref() == "_" {
                        targets.push(HirDestructureTarget {
                            role: HirDestructureTargetRole::Discard,
                            local: None,
                            component: u32::try_from(component_index).unwrap_or(u32::MAX),
                            ty: component_ty,
                            name_origin,
                            origin: target_origin,
                        });
                        continue;
                    }
                    let target_binding = self.active_binding(name.as_ref());
                    match target_binding {
                        Some(ActiveBinding::Unique(binding)) => {
                            if binding.kind == HirBindingKind::Const {
                                self.diagnostics.push(
                                    PendingDiagnostic::new(
                                        IMMUTABLE_ASSIGNMENT,
                                        format!("cannot assign to immutable binding `{name}`"),
                                        ast_target.name.span,
                                    )
                                    .primary("binding is `const`"),
                                );
                                valid = false;
                            }
                            if binding.ty != component_ty {
                                self.type_mismatch(
                                    ast_target.name.span,
                                    component_ty,
                                    binding.ty,
                                    binding.type_span,
                                    "expected type from destructure target",
                                );
                                valid = false;
                            }
                            assigned.set(binding.local, true);
                            targets.push(HirDestructureTarget {
                                role: HirDestructureTargetRole::Assign,
                                local: Some(binding.local),
                                component: u32::try_from(component_index).unwrap_or(u32::MAX),
                                ty: component_ty,
                                name_origin,
                                origin: target_origin,
                            });
                        }
                        Some(ActiveBinding::Poisoned(_)) => {
                            valid = false;
                        }
                        None => {
                            self.diagnostics.push(
                                PendingDiagnostic::new(
                                    UNKNOWN_NAME,
                                    format!("unknown binding `{name}`"),
                                    ast_target.name.span,
                                )
                                .primary("no active binding has this name"),
                            );
                            valid = false;
                        }
                    }
                }
            }
        }

        let Some(operand) = operand.expression else {
            return Ok(CheckedStatement::invalid());
        };
        if !valid {
            return Ok(CheckedStatement::invalid());
        }
        Ok(CheckedStatement::continuing(HirStatement {
            kind: HirStatementKind::Destructure {
                operand,
                targets: targets.into_boxed_slice(),
            },
            origin: self.origin(statement.span)?,
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
        let outer_loop_depth = std::mem::replace(&mut self.loop_depth, 0);
        let body = self.check_block(block, assigned);
        self.loop_depth = outer_loop_depth;
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

    #[allow(
        clippy::too_many_lines,
        reason = "the declaration checker handles annotated, inferred `:=`, and destructuring forms in one exhaustive pass"
    )]
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
        let declared_type = if let Some(ty) = &declaration.ty {
            self.resolve_type(ty.kind.clone())
                .unwrap_or(ValueType::Int32)
        } else {
            let Some(initializer) = declaration.initializer.as_ref() else {
                self.diagnostics.push(PendingDiagnostic::new(
                    DIRTY_AST,
                    "inferred declaration without an initializer reached semantic checking",
                    declaration.span,
                ));
                return Ok(CheckedStatement::invalid());
            };
            let checked = self.check_expression(initializer, assigned)?;
            let Some(ty) = checked.ty else {
                return Ok(CheckedStatement::invalid());
            };
            ty
        };
        let type_span = declaration
            .ty
            .as_ref()
            .map_or(declaration.name.span, |ty| ty.span);
        let local = self.allocate_binding(
            binding_kind,
            declared_type,
            declaration.name.span,
            type_span,
            declaration.span,
        )?;

        let initializer = declaration
            .initializer
            .as_ref()
            .map(|initializer| {
                self.check_expression_expected(initializer, assigned, Some(declared_type))
            })
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
                        type_span,
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
            type_span,
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

    fn resolve_type(&mut self, kind: AstValueTypeKind) -> Option<ValueType> {
        resolve_value_type(
            self.sources,
            self.signature.module,
            kind,
            &self.signatures.types_by_module,
            &self.signatures.anonymous,
            self.diagnostics,
        )
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

        let expected = target.map(|target| target.original().ty);
        let value = self.check_expression_expected(&assignment.value, assigned, expected)?;
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

    fn check_switch_statement(
        &mut self,
        switch: &AstSwitchStatement,
        assigned: &mut Assigned,
    ) -> Result<CheckedStatement, CheckError> {
        let scrutinee = self.check_expression(&switch.scrutinee, assigned)?;
        let Some(scrutinee_ty) = scrutinee.ty else {
            return Ok(CheckedStatement::invalid());
        };
        if !matches!(scrutinee_ty, ValueType::Int32 | ValueType::Enum(_)) {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_SWITCH_PATTERN,
                    "switch scrutinee must be Int32 or an enum",
                    switch.scrutinee.span,
                )
                .primary("unsupported switch type"),
            );
        }
        let mut coverage = SwitchCoverage::new(scrutinee_ty, self.enum_variant_count(scrutinee_ty));
        let checkpoint = assigned.checkpoint();
        let mut continuing_delta = None;
        let mut arms = vec![];
        let mut valid = scrutinee.expression.is_some()
            && matches!(scrutinee_ty, ValueType::Int32 | ValueType::Enum(_));
        for (index, arm) in switch.arms.iter().enumerate() {
            let label = self.check_switch_label(
                &arm.label,
                scrutinee_ty,
                &mut coverage,
                index + 1 == switch.arms.len(),
            )?;
            let body = self.check_block(&arm.body, assigned)?;
            if body.continues {
                let delta = assigned.newly_assigned_since(checkpoint);
                Assigned::merge_delta_intersection(&mut continuing_delta, &delta);
            }
            assigned.rollback(checkpoint);
            valid &= label.is_some() && body.valid;
            if let Some(label) = label {
                arms.push(HirSwitchStatementArm {
                    label,
                    body: body.block,
                    origin: self.origin(arm.span)?,
                });
            }
        }
        valid &= self.finish_switch_coverage(&coverage, switch.span);
        if let Some(delta) = continuing_delta.as_ref() {
            for index in delta {
                assigned.set_index(*index, true);
            }
        }
        let continues = continuing_delta.is_some();
        let kind = match (valid, scrutinee.expression) {
            (true, Some(scrutinee)) => Some(HirStatementKind::Switch(HirSwitchStatement {
                scrutinee,
                arms: arms.into_boxed_slice(),
                origin: self.origin(switch.span)?,
            })),
            _ => None,
        };
        self.finish_statement(kind, switch.span, continues)
    }

    fn check_switch_expression(
        &mut self,
        switch: &AstSwitchExpression,
        assigned: &Assigned,
        expected: Option<ValueType>,
    ) -> Result<CheckedExpression, CheckError> {
        let scrutinee = self.check_expression(&switch.scrutinee, assigned)?;
        let Some(scrutinee_ty) = scrutinee.ty else {
            return Ok(CheckedExpression::invalid(switch.span));
        };
        let mut valid = scrutinee.expression.is_some()
            && matches!(scrutinee_ty, ValueType::Int32 | ValueType::Enum(_));
        if !matches!(scrutinee_ty, ValueType::Int32 | ValueType::Enum(_)) {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_SWITCH_PATTERN,
                    "switch scrutinee must be Int32 or an enum",
                    switch.scrutinee.span,
                )
                .primary("unsupported switch type"),
            );
        }
        let mut coverage = SwitchCoverage::new(scrutinee_ty, self.enum_variant_count(scrutinee_ty));
        let mut result_ty = expected;
        let mut arms = vec![];
        for (index, arm) in switch.arms.iter().enumerate() {
            let label = self.check_switch_label(
                &arm.label,
                scrutinee_ty,
                &mut coverage,
                index + 1 == switch.arms.len(),
            )?;
            let body = self.check_expression_expected(&arm.body, assigned, result_ty)?;
            if result_ty.is_none() {
                result_ty = body.ty;
            }
            if let (Some(actual), Some(expected)) = (body.ty, result_ty) {
                if actual != expected {
                    self.type_mismatch_without_support(
                        body.span,
                        actual,
                        expected,
                        "all switch expression arms must have the same type",
                    );
                    valid = false;
                }
            }
            valid &= label.is_some() && body.expression.is_some();
            if let (Some(label), Some(body)) = (label, body.expression) {
                arms.push(HirSwitchExpressionArm {
                    label,
                    body,
                    origin: self.origin(arm.span)?,
                });
            }
        }
        valid &= self.finish_switch_coverage(&coverage, switch.span);
        let Some(result_ty) = result_ty else {
            self.diagnostics.push(PendingDiagnostic::new(
                NON_EXHAUSTIVE_SWITCH,
                "switch expression has no typed arms",
                switch.span,
            ));
            return Ok(CheckedExpression::invalid(switch.span));
        };
        let Some(scrutinee) = scrutinee.expression else {
            return Ok(CheckedExpression::invalid(switch.span));
        };
        if !valid {
            return Ok(CheckedExpression::invalid(switch.span));
        }
        Ok(CheckedExpression::valid(
            HirExpressionKind::Switch(HirSwitchExpression {
                scrutinee: Box::new(scrutinee),
                arms: arms.into_boxed_slice(),
                origin: self.origin(switch.span)?,
            }),
            result_ty,
            self.origin(switch.span)?,
            switch.span,
        ))
    }

    fn enum_variant_count(&self, ty: ValueType) -> usize {
        let ValueType::Enum(id) = ty else {
            return 0;
        };
        id.as_usize()
            .and_then(|index| self.signatures.enums.get(index))
            .map_or(0, |enum_| enum_.variants.len())
    }

    fn check_switch_label(
        &mut self,
        label: &AstSwitchLabel,
        scrutinee_ty: ValueType,
        coverage: &mut SwitchCoverage,
        is_last: bool,
    ) -> Result<Option<HirSwitchLabel>, CheckError> {
        match label {
            AstSwitchLabel::Else(span) => {
                if coverage.else_span.is_some() || !is_last || coverage.is_complete() {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            INVALID_SWITCH_ELSE,
                            "`else` must appear once, last, and only when coverage is incomplete",
                            *span,
                        )
                        .primary("invalid switch else arm"),
                    );
                    return Ok(None);
                }
                coverage.else_span = Some(*span);
                Ok(Some(HirSwitchLabel::Else))
            }
            AstSwitchLabel::Patterns(patterns) => {
                if coverage.else_span.is_some() {
                    if let Some(pattern) = patterns.first() {
                        self.diagnostics.push(PendingDiagnostic::new(
                            INVALID_SWITCH_ELSE,
                            "patterns cannot follow `else`",
                            pattern.span,
                        ));
                    }
                    return Ok(None);
                }
                let mut hir = vec![];
                let mut valid = !patterns.is_empty();
                for pattern in patterns {
                    if let Some(pattern) =
                        self.check_switch_pattern(pattern, scrutinee_ty, coverage)?
                    {
                        hir.push(pattern);
                    } else {
                        valid = false;
                    }
                }
                Ok(valid.then(|| HirSwitchLabel::Patterns(hir.into_boxed_slice())))
            }
        }
    }

    fn check_switch_pattern(
        &mut self,
        pattern: &AstSwitchPattern,
        scrutinee_ty: ValueType,
        coverage: &mut SwitchCoverage,
    ) -> Result<Option<HirSwitchPattern>, CheckError> {
        let kind = match (&pattern.kind, scrutinee_ty) {
            (AstSwitchPatternKind::Integer(value), ValueType::Int32) => {
                let Some(value) = self.parse_signed_integer(value) else {
                    return Ok(None);
                };
                if let Some(previous) = coverage.insert_interval(value, value, pattern.span) {
                    self.overlap_diagnostic(pattern.span, previous);
                    return Ok(None);
                }
                HirSwitchPatternKind::IntRange {
                    min: value,
                    max: value,
                }
            }
            (AstSwitchPatternKind::IntegerRange { min, max }, ValueType::Int32) => {
                let (Some(min), Some(max)) = (
                    self.parse_signed_integer(min),
                    self.parse_signed_integer(max),
                ) else {
                    return Ok(None);
                };
                if min > max {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            INVALID_SWITCH_PATTERN,
                            "switch range lower bound exceeds its upper bound",
                            pattern.span,
                        )
                        .primary("backwards inclusive range"),
                    );
                    return Ok(None);
                }
                if let Some(previous) = coverage.insert_interval(min, max, pattern.span) {
                    self.overlap_diagnostic(pattern.span, previous);
                    return Ok(None);
                }
                HirSwitchPatternKind::IntRange { min, max }
            }
            (AstSwitchPatternKind::EnumVariant { ty, variant }, ValueType::Enum(enum_id)) => {
                if let Some(ty) = ty {
                    let resolved = self.resolve_type(AstValueTypeKind::Named(*ty));
                    if resolved != Some(ValueType::Enum(enum_id)) {
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                INVALID_SWITCH_PATTERN,
                                "qualified enum pattern has the wrong nominal type",
                                ty.span,
                            )
                            .primary("expected the scrutinee enum type"),
                        );
                        return Ok(None);
                    }
                }
                let Some(variant_id) = self.resolve_enum_variant(enum_id, variant.span)? else {
                    return Ok(None);
                };
                let index = variant_id.as_usize().unwrap_or(usize::MAX);
                if let Some(previous) = coverage.insert_variant(index, pattern.span) {
                    self.overlap_diagnostic(pattern.span, previous);
                    return Ok(None);
                }
                HirSwitchPatternKind::EnumVariant {
                    enum_: enum_id,
                    variant: variant_id,
                }
            }
            _ => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        INVALID_SWITCH_PATTERN,
                        format!("pattern does not match switch type {scrutinee_ty}"),
                        pattern.span,
                    )
                    .primary("wrong pattern kind"),
                );
                return Ok(None);
            }
        };
        Ok(Some(HirSwitchPattern {
            kind,
            origin: self.origin(pattern.span)?,
        }))
    }

    fn parse_signed_integer(&mut self, literal: &super::ast::AstSignedInteger) -> Option<i32> {
        let digits = self.sources.files().slice(literal.digits).ok()?;
        let spelling = if literal.negative {
            format!("-{digits}")
        } else {
            digits.to_owned()
        };
        if let Ok(value) = spelling.parse::<i32>() {
            Some(value)
        } else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INTEGER_OUT_OF_RANGE,
                    "switch pattern integer is outside the Int32 range",
                    literal.span,
                )
                .primary("expected a signed 32-bit integer"),
            );
            None
        }
    }

    fn overlap_diagnostic(&mut self, span: Span, previous: Span) {
        self.diagnostics.push(
            PendingDiagnostic::new(OVERLAPPING_SWITCH_PATTERN, "switch patterns overlap", span)
                .primary("overlapping pattern")
                .support(previous, "earlier covering pattern is here"),
        );
    }

    fn finish_switch_coverage(&mut self, coverage: &SwitchCoverage, span: Span) -> bool {
        if coverage.else_span.is_some() || coverage.is_complete() {
            return true;
        }
        let message = match coverage.ty {
            ValueType::Enum(enum_id) => {
                let missing = enum_id
                    .as_usize()
                    .and_then(|index| self.signatures.enums.get(index))
                    .map(|enum_| {
                        enum_
                            .variants
                            .iter()
                            .enumerate()
                            .filter(|(index, _)| {
                                coverage.enum_seen.get(*index).is_some_and(Option::is_none)
                            })
                            .map(|(_, variant)| variant.name.as_ref())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                format!("non-exhaustive enum switch; missing: {missing}")
            }
            _ => "non-exhaustive Int32 switch; add `else` or cover the full domain".to_owned(),
        };
        self.diagnostics.push(
            PendingDiagnostic::new(NON_EXHAUSTIVE_SWITCH, message, span)
                .primary("switch is not exhaustive"),
        );
        false
    }

    fn resolve_enum_variant(
        &mut self,
        enum_id: SourceEnumId,
        variant_span: Span,
    ) -> Result<Option<SourceVariantId>, CheckError> {
        let spelling = self.spelling(variant_span)?;
        let resolved = enum_id
            .as_usize()
            .and_then(|index| self.signatures.enums.get(index))
            .and_then(|enum_| {
                enum_
                    .variants
                    .iter()
                    .find(|variant| variant.name.as_ref() == spelling.as_ref())
            })
            .map(|variant| variant.id);
        if resolved.is_none() {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNKNOWN_ENUM_VARIANT,
                    format!("enum has no variant `{spelling}`"),
                    variant_span,
                )
                .primary("unknown enum variant"),
            );
        }
        Ok(resolved)
    }

    fn check_enum_literal(
        &mut self,
        qualified: Option<AstName>,
        variant: AstName,
        expected: Option<ValueType>,
        span: Span,
    ) -> Result<CheckedExpression, CheckError> {
        let enum_id = if let Some(ty) = qualified {
            match self.resolve_type(AstValueTypeKind::Named(ty)) {
                Some(ValueType::Enum(id)) => Some(id),
                _ => None,
            }
        } else if let Some(ValueType::Enum(id)) = expected {
            Some(id)
        } else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    ENUM_CONTEXT_REQUIRED,
                    "inferred enum literal requires an exact expected enum type",
                    span,
                )
                .primary("write a qualified variant or provide a typed context"),
            );
            None
        };
        let Some(enum_id) = enum_id else {
            return Ok(CheckedExpression::invalid(span));
        };
        let Some(variant) = self.resolve_enum_variant(enum_id, variant.span)? else {
            return Ok(CheckedExpression::invalid(span));
        };
        Ok(CheckedExpression::valid(
            HirExpressionKind::EnumVariant {
                enum_: enum_id,
                variant,
            },
            ValueType::Enum(enum_id),
            self.origin(span)?,
            span,
        ))
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
            .map(|value| {
                self.check_expression_expected(
                    value,
                    assigned,
                    match self.signature.result {
                        FunctionResult::Value(ty) => Some(ty),
                        FunctionResult::Void => None,
                    },
                )
            })
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

    #[allow(
        clippy::too_many_lines,
        reason = "the closed expression syntax is checked exhaustively at one dispatch boundary"
    )]
    fn check_expression(
        &mut self,
        expression: &AstExpression,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        self.check_expression_expected(expression, assigned, None)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one exhaustive typed expression boundary keeps contextual enum inference explicit"
    )]
    fn check_expression_expected(
        &mut self,
        expression: &AstExpression,
        assigned: &Assigned,
        expected: Option<ValueType>,
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
            AstExpressionKind::StringLiteral(literal) => {
                let Some(value) = decode_string_literal(self.sources, *literal)? else {
                    return Ok(CheckedExpression::invalid(expression.span));
                };
                Ok(CheckedExpression::valid(
                    HirExpressionKind::String {
                        op: HirStringOp::Constant(value),
                        operands: Box::new([]),
                    },
                    ValueType::String,
                    self.origin(expression.span)?,
                    expression.span,
                ))
            }
            AstExpressionKind::Name(name) => {
                self.check_name_expression(*name, expression.span, assigned)
            }
            AstExpressionKind::InferredEnumLiteral(variant) => {
                self.check_enum_literal(None, *variant, expected, expression.span)
            }
            AstExpressionKind::Switch(switch) => {
                self.check_switch_expression(switch, assigned, expected)
            }
            AstExpressionKind::Member {
                receiver, member, ..
            } => {
                if self.expression_roots_in_nbt_path_receiver(expression) {
                    return self.check_entity_nbt_path_expression(expression, assigned);
                }
                self.check_member_expression(receiver, member.span, expression.span, assigned)
            }
            AstExpressionKind::MemberKey { .. } => {
                if self.expression_roots_in_nbt_path_receiver(expression) {
                    return self.check_entity_nbt_path_expression(expression, assigned);
                }
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        UNRESOLVED_MEMBER,
                        "a string-literal member key is only valid on a schema-typed entity-NBT path",
                        expression.span,
                    )
                    .primary("this receiver does not support compile-time compound keys"),
                );
                Ok(CheckedExpression::invalid(expression.span))
            }
            AstExpressionKind::Call(call) => {
                if let Some(checked) = self.check_length_call(call, assigned)? {
                    return Ok(checked);
                }
                if let Some(checked) = self.check_string_call(call, assigned)? {
                    return Ok(checked);
                }
                if let Some(checked) = self.check_list_i32_call(call, assigned)? {
                    return Ok(checked);
                }
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
                    (
                        Some(CheckedCallTarget::External(external)),
                        Some(FunctionResult::Value(ty)),
                    ) => {
                        let origin = self
                            .external_ops
                            .get(
                                external
                                    .as_usize()
                                    .expect("allocated external ID fits usize"),
                            )
                            .expect("checked external call was just allocated")
                            .origin;
                        Ok(CheckedExpression::valid(
                            HirExpressionKind::External(external),
                            ty,
                            origin,
                            expression.span,
                        ))
                    }
                    _ => Ok(CheckedExpression::invalid(expression.span)),
                }
            }
            AstExpressionKind::StructLiteral(literal) => {
                self.check_struct_literal(literal, expression.span, assigned)
            }
            AstExpressionKind::Not(operand) => {
                self.check_not_expression(operand, expression.span, assigned)
            }
            AstExpressionKind::WrappingArithmetic { op, left, right } => {
                self.check_wrapping_arithmetic(*op, left, right, expression.span, assigned)
            }
            AstExpressionKind::Compare { op, left, right } => {
                self.check_comparison(*op, left, right, expression.span, assigned)
            }
            AstExpressionKind::Index { aggregate, index } => {
                if self.expression_roots_in_nbt_path_receiver(expression) {
                    return self.check_entity_nbt_path_expression(expression, assigned);
                }
                self.check_index_expression(aggregate, index, expression.span, assigned)
            }
            AstExpressionKind::InferredStructLiteral(literal) => {
                self.check_inferred_struct_literal(literal, expression.span, assigned, expected)
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

    fn check_length_call(
        &mut self,
        call: &AstCall,
        assigned: &Assigned,
    ) -> Result<Option<CheckedExpression>, CheckError> {
        let AstExpressionKind::Member {
            receiver, member, ..
        } = &call.callee.kind
        else {
            return Ok(None);
        };
        if self.spelling(member.span)?.as_ref() != "length" {
            return Ok(None);
        }

        let receiver = self.check_expression(receiver, assigned)?;
        let kind = match receiver.ty {
            Some(ValueType::String) => {
                receiver
                    .expression
                    .map(|receiver| HirExpressionKind::String {
                        op: HirStringOp::Length,
                        operands: vec![receiver].into_boxed_slice(),
                    })
            }
            Some(ValueType::ListI32) => {
                receiver
                    .expression
                    .map(|receiver| HirExpressionKind::ListI32 {
                        op: HirListI32Op::Length,
                        operands: vec![receiver].into_boxed_slice(),
                    })
            }
            Some(actual) => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        TYPE_MISMATCH,
                        format!("type {actual} has no `length` method"),
                        receiver.span,
                    )
                    .primary("`length` requires String or List<Int32>"),
                );
                None
            }
            None => None,
        };
        if !call.arguments.is_empty() {
            self.invalid_list_arity(call.span, "length", 0, call.arguments.len());
            for argument in &call.arguments {
                let _ = self.check_expression(argument, assigned)?;
            }
            return Ok(Some(CheckedExpression::invalid(call.span)));
        }
        let Some(kind) = kind else {
            return Ok(Some(CheckedExpression::invalid(call.span)));
        };
        Ok(Some(CheckedExpression::valid(
            kind,
            ValueType::Int32,
            self.origin(call.span)?,
            call.span,
        )))
    }

    fn check_string_call(
        &mut self,
        call: &AstCall,
        assigned: &Assigned,
    ) -> Result<Option<CheckedExpression>, CheckError> {
        let AstExpressionKind::Member {
            receiver, member, ..
        } = &call.callee.kind
        else {
            return Ok(None);
        };
        let method = self.spelling(member.span)?;
        let (op, expected_arguments, result) = match method.as_ref() {
            "without_last_unit" => (HirStringOp::WithoutLastUnit, 0, ValueType::String),
            "ends_with_ascii" => {
                if call.arguments.len() != 1 {
                    self.invalid_list_arity(call.span, &method, 1, call.arguments.len());
                    let _ = self.check_expression(receiver, assigned)?;
                    return Ok(Some(CheckedExpression::invalid(call.span)));
                }
                let AstExpressionKind::StringLiteral(literal) = call.arguments[0].kind else {
                    let _ = self.check_expression(&call.arguments[0], assigned)?;
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            LITERAL_CONTEXT_REQUIRED,
                            "ends_with_ascii requires one static single-byte ASCII string literal",
                            call.arguments[0].span,
                        )
                        .primary("runtime command text never enters string-slice syntax"),
                    );
                    return Ok(Some(CheckedExpression::invalid(call.span)));
                };
                let Some(value) = decode_string_literal(self.sources, literal)? else {
                    return Ok(Some(CheckedExpression::invalid(call.span)));
                };
                let bytes = value.as_bytes();
                if bytes.len() != 1 || !bytes[0].is_ascii() {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            LITERAL_CONTEXT_REQUIRED,
                            "ends_with_ascii requires exactly one ASCII character",
                            literal,
                        )
                        .primary("this literal is not one ASCII byte"),
                    );
                    return Ok(Some(CheckedExpression::invalid(call.span)));
                }
                (HirStringOp::EndsWithAscii(bytes[0]), 1, ValueType::Bool)
            }
            _ => return Ok(None),
        };
        let receiver = self.check_expression(receiver, assigned)?;
        let mut valid = receiver.ty == Some(ValueType::String) && receiver.expression.is_some();
        if let Some(actual) = receiver.ty {
            if actual != ValueType::String {
                self.type_mismatch_without_support(
                    receiver.span,
                    actual,
                    ValueType::String,
                    "this method requires a String receiver",
                );
            }
        }
        if call.arguments.len() != expected_arguments {
            self.invalid_list_arity(call.span, &method, expected_arguments, call.arguments.len());
            valid = false;
        }
        Ok(Some(if valid {
            CheckedExpression::valid(
                HirExpressionKind::String {
                    op,
                    operands: receiver
                        .expression
                        .into_iter()
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                },
                result,
                self.origin(call.span)?,
                call.span,
            )
        } else {
            CheckedExpression::invalid(call.span)
        }))
    }

    fn check_list_i32_call(
        &mut self,
        call: &AstCall,
        assigned: &Assigned,
    ) -> Result<Option<CheckedExpression>, CheckError> {
        let AstExpressionKind::Member {
            receiver, member, ..
        } = &call.callee.kind
        else {
            return Ok(None);
        };
        let method = self.spelling(member.span)?;
        if let AstExpressionKind::Name(root) = receiver.kind {
            if self.spelling(root.span)?.as_ref() == "List" && method.as_ref() == "empty" {
                if !call.arguments.is_empty() {
                    self.invalid_list_arity(call.span, "List.empty", 0, call.arguments.len());
                    for argument in &call.arguments {
                        let _ = self.check_expression(argument, assigned)?;
                    }
                    return Ok(Some(CheckedExpression::invalid(call.span)));
                }
                return Ok(Some(CheckedExpression::valid(
                    HirExpressionKind::ListI32 {
                        op: HirListI32Op::Empty,
                        operands: Box::new([]),
                    },
                    ValueType::ListI32,
                    self.origin(call.span)?,
                    call.span,
                )));
            }
        }

        let (op, argument_count, result) = match method.as_ref() {
            "push" => (HirListI32Op::Push, 1, ValueType::ListI32),
            "last_or_zero" => (HirListI32Op::LastOrZero, 0, ValueType::Int32),
            "without_last" => (HirListI32Op::WithoutLast, 0, ValueType::ListI32),
            _ => return Ok(None),
        };
        let receiver = self.check_expression(receiver, assigned)?;
        let mut valid = receiver.ty == Some(ValueType::ListI32) && receiver.expression.is_some();
        if let Some(actual) = receiver.ty {
            if actual != ValueType::ListI32 {
                self.type_mismatch_without_support(
                    receiver.span,
                    actual,
                    ValueType::ListI32,
                    "this method requires a List<Int32> receiver",
                );
            }
        }
        if call.arguments.len() != argument_count {
            self.invalid_list_arity(call.span, &method, argument_count, call.arguments.len());
            valid = false;
        }
        let mut operands = Vec::with_capacity(1 + call.arguments.len());
        if let Some(receiver) = receiver.expression {
            operands.push(receiver);
        }
        for argument in &call.arguments {
            let argument = self.check_expression(argument, assigned)?;
            if argument.ty != Some(ValueType::Int32) {
                if let Some(actual) = argument.ty {
                    self.type_mismatch_without_support(
                        argument.span,
                        actual,
                        ValueType::Int32,
                        "list push requires an Int32 element",
                    );
                }
                valid = false;
            }
            if let Some(argument) = argument.expression {
                operands.push(argument);
            } else {
                valid = false;
            }
        }
        Ok(Some(if valid {
            CheckedExpression::valid(
                HirExpressionKind::ListI32 {
                    op,
                    operands: operands.into_boxed_slice(),
                },
                result,
                self.origin(call.span)?,
                call.span,
            )
        } else {
            CheckedExpression::invalid(call.span)
        }))
    }

    fn invalid_list_arity(&mut self, span: Span, method: &str, expected: usize, actual: usize) {
        self.diagnostics.push(
            PendingDiagnostic::new(
                ARGUMENT_COUNT,
                format!(
                    "list method `{method}` expects {expected} arguments but received {actual}"
                ),
                span,
            )
            .primary("argument count does not match the list operation"),
        );
    }

    #[allow(
        clippy::too_many_lines,
        reason = "context-inferred struct literals validate every named and positional entry form in one pass"
    )]
    fn check_inferred_struct_literal(
        &mut self,
        literal: &AstInferredStructLiteral,
        span: Span,
        assigned: &Assigned,
        expected: Option<ValueType>,
    ) -> Result<CheckedExpression, CheckError> {
        let Some(expected) = expected else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    LITERAL_CONTEXT_REQUIRED,
                    "inferred struct literal requires an exact expected type",
                    span,
                )
                .primary("write an explicit type annotation or an explicit struct literal"),
            );
            return Ok(CheckedExpression::invalid(span));
        };
        match &literal.entries {
            AstInferredStructEntries::Named(fields) => {
                let anon_id: Option<SourceAnonymousStructId>;
                let nom_id: Option<SourceStructId>;
                let field_specs: Vec<(Box<str>, ValueType, Span)>;
                match expected {
                    ValueType::Struct(id) => {
                        let struct_ = id.as_usize().and_then(|i| self.signatures.structs.get(i));
                        let Some(struct_) = struct_ else {
                            return Ok(CheckedExpression::invalid(span));
                        };
                        field_specs = struct_
                            .fields
                            .iter()
                            .map(|f| (f.name.clone(), f.ty, f.name_span))
                            .collect();
                        anon_id = None;
                        nom_id = Some(id);
                    }
                    ValueType::AnonymousStruct(id) => {
                        let interner = self.signatures.anonymous.borrow();
                        let anonymous = interner.entries.iter().find(|e| e.id == id);
                        let Some(anonymous) = anonymous else {
                            return Ok(CheckedExpression::invalid(span));
                        };
                        let AnonymousTypeKey::Named(field_list) = &anonymous.key else {
                            self.diagnostics.push(
                                PendingDiagnostic::new(
                                    TYPE_MISMATCH,
                                    "named inferred struct literal requires a named anonymous struct",
                                    span,
                                )
                                .primary("expected a named anonymous struct type"),
                            );
                            return Ok(CheckedExpression::invalid(span));
                        };
                        field_specs = field_list
                            .iter()
                            .map(|(name, ty)| (name.clone(), *ty, span))
                            .collect();
                        anon_id = Some(id);
                        nom_id = None;
                    }
                    _ => {
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                TYPE_MISMATCH,
                                "inferred struct literal with named entries requires a struct type",
                                span,
                            )
                            .primary("expected a struct or named anonymous struct type"),
                        );
                        return Ok(CheckedExpression::invalid(span));
                    }
                }
                let mut seen = vec![None::<Span>; field_specs.len()];
                let mut values = Vec::with_capacity(fields.len());
                let mut valid = true;
                for initializer in fields {
                    let spelling = self.spelling(initializer.name.span)?;
                    let index = field_specs
                        .iter()
                        .position(|(name, _, _)| name.as_ref() == spelling.as_ref());
                    let checked = self.check_expression_expected(
                        &initializer.value,
                        assigned,
                        index.map(|i| field_specs[i].1),
                    )?;
                    let Some(index) = index else {
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                UNKNOWN_STRUCT_FIELD,
                                format!("struct has no field `{spelling}`"),
                                initializer.name.span,
                            )
                            .primary("unknown field initializer"),
                        );
                        valid = false;
                        continue;
                    };
                    if let Some(original) = seen[index].replace(initializer.name.span) {
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                DUPLICATE_STRUCT_INITIALIZER,
                                format!("field `{spelling}` is initialized more than once"),
                                initializer.name.span,
                            )
                            .primary("duplicate field initializer")
                            .support(original, "first initializer is here"),
                        );
                        valid = false;
                    }
                    if checked.ty != Some(field_specs[index].1) {
                        if let Some(actual) = checked.ty {
                            self.type_mismatch(
                                checked.span,
                                actual,
                                field_specs[index].1,
                                field_specs[index].2,
                                "field type is declared here",
                            );
                        }
                        valid = false;
                    }
                    if let Some(value) = checked.expression {
                        values.push(HirStructFieldValue {
                            field: u32::try_from(index).unwrap_or(u32::MAX),
                            value,
                            origin: self.origin(initializer.span)?,
                        });
                    } else {
                        valid = false;
                    }
                }
                for (index, occurrence) in seen.iter().enumerate() {
                    if occurrence.is_none() {
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                MISSING_STRUCT_FIELD,
                                format!("missing initializer for field `{}`", field_specs[index].0),
                                span,
                            )
                            .primary("every field must be initialized")
                            .support(field_specs[index].2, "field is declared here"),
                        );
                        valid = false;
                    }
                }
                if !valid {
                    return Ok(CheckedExpression::invalid(span));
                }
                if let Some(id) = anon_id {
                    Ok(CheckedExpression::valid(
                        HirExpressionKind::AnonymousStructConstruct {
                            struct_: id,
                            fields: values.into_boxed_slice(),
                        },
                        ValueType::AnonymousStruct(id),
                        self.origin(span)?,
                        span,
                    ))
                } else {
                    Ok(CheckedExpression::valid(
                        HirExpressionKind::StructConstruct {
                            struct_: nom_id.unwrap(),
                            fields: values.into_boxed_slice(),
                        },
                        ValueType::Struct(nom_id.unwrap()),
                        self.origin(span)?,
                        span,
                    ))
                }
            }
            AstInferredStructEntries::Positional(values) => {
                let ValueType::AnonymousStruct(struct_id) = expected else {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            TYPE_MISMATCH,
                            "inferred struct literal with positional entries requires a positional anonymous struct",
                            span,
                        )
                        .primary("expected a positional anonymous struct type"),
                    );
                    return Ok(CheckedExpression::invalid(span));
                };
                let interner = self.signatures.anonymous.borrow();
                let anonymous = interner.entries.iter().find(|e| e.id == struct_id);
                let Some(anonymous) = anonymous else {
                    return Ok(CheckedExpression::invalid(span));
                };
                let AnonymousTypeKey::Positional(components) = &anonymous.key else {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            TYPE_MISMATCH,
                            "inferred struct literal with positional entries requires a positional anonymous struct",
                            span,
                        )
                        .primary("expected a positional anonymous struct type"),
                    );
                    return Ok(CheckedExpression::invalid(span));
                };
                if values.len() != components.len() {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            ARGUMENT_COUNT,
                            format!(
                                "inferred struct literal requires {} entries but {} were provided",
                                components.len(),
                                values.len(),
                            ),
                            span,
                        )
                        .primary("entry count must match the anonymous struct's component count"),
                    );
                    return Ok(CheckedExpression::invalid(span));
                }
                let mut checked = Vec::with_capacity(values.len());
                let mut valid = true;
                for (index, value) in values.iter().enumerate() {
                    let entry =
                        self.check_expression_expected(value, assigned, Some(components[index]))?;
                    if entry.ty != Some(components[index]) {
                        valid = false;
                    }
                    if let Some(expr) = entry.expression {
                        checked.push(HirStructFieldValue {
                            field: u32::try_from(index).unwrap_or(u32::MAX),
                            value: expr,
                            origin: self.origin(value.span)?,
                        });
                    } else {
                        valid = false;
                    }
                }
                if !valid {
                    return Ok(CheckedExpression::invalid(span));
                }
                Ok(CheckedExpression::valid(
                    HirExpressionKind::AnonymousStructConstruct {
                        struct_: struct_id,
                        fields: checked.into_boxed_slice(),
                    },
                    ValueType::AnonymousStruct(struct_id),
                    self.origin(span)?,
                    span,
                ))
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "nominal field checking retains exact duplicate, missing, and type diagnostics"
    )]
    fn check_struct_literal(
        &mut self,
        literal: &AstStructLiteral,
        span: Span,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        let Some(ValueType::Struct(struct_id)) =
            self.resolve_type(AstValueTypeKind::Named(literal.ty))
        else {
            for field in &literal.fields {
                let _ = self.check_expression(&field.value, assigned)?;
            }
            return Ok(CheckedExpression::invalid(span));
        };
        let Some(struct_) = self.signatures.structs.get(struct_id.as_usize().ok_or(
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::Struct),
        )?) else {
            return Ok(CheckedExpression::invalid(span));
        };
        let field_specs = struct_
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.ty, field.name_span))
            .collect::<Vec<_>>();
        let mut seen = vec![None::<Span>; field_specs.len()];
        let mut values = Vec::with_capacity(literal.fields.len());
        let mut valid = true;
        for initializer in &literal.fields {
            let spelling = self.spelling(initializer.name.span)?;
            let index = field_specs
                .iter()
                .position(|(name, _, _)| name.as_ref() == spelling.as_ref());
            let checked = self.check_expression_expected(
                &initializer.value,
                assigned,
                index.map(|index| field_specs[index].1),
            )?;
            let Some(index) = index else {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        UNKNOWN_STRUCT_FIELD,
                        format!("struct has no field `{spelling}`"),
                        initializer.name.span,
                    )
                    .primary("unknown field initializer"),
                );
                valid = false;
                continue;
            };
            if let Some(original) = seen[index].replace(initializer.name.span) {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        DUPLICATE_STRUCT_INITIALIZER,
                        format!("field `{spelling}` is initialized more than once"),
                        initializer.name.span,
                    )
                    .primary("duplicate field initializer")
                    .support(original, "first initializer is here"),
                );
                valid = false;
            }
            if checked.ty != Some(field_specs[index].1) {
                if let Some(actual) = checked.ty {
                    self.type_mismatch(
                        checked.span,
                        actual,
                        field_specs[index].1,
                        field_specs[index].2,
                        "field type is declared here",
                    );
                }
                valid = false;
            }
            if let Some(value) = checked.expression {
                values.push(HirStructFieldValue {
                    field: u32::try_from(index).unwrap_or(u32::MAX),
                    value,
                    origin: self.origin(initializer.span)?,
                });
            } else {
                valid = false;
            }
        }
        for (index, occurrence) in seen.iter().enumerate() {
            if occurrence.is_none() {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        MISSING_STRUCT_FIELD,
                        format!("missing initializer for field `{}`", field_specs[index].0),
                        literal.ty.span,
                    )
                    .primary("every struct field must be initialized")
                    .support(field_specs[index].2, "field is declared here"),
                );
                valid = false;
            }
        }
        if !valid {
            return Ok(CheckedExpression::invalid(span));
        }
        Ok(CheckedExpression::valid(
            HirExpressionKind::StructConstruct {
                struct_: struct_id,
                fields: values.into_boxed_slice(),
            },
            ValueType::Struct(struct_id),
            self.origin(span)?,
            span,
        ))
    }

    /// Returns true iff `expression`'s postfix spine is rooted in an active
    /// executor capture name (`reader`, `reader.equipment`,
    /// `reader.equipment[0]`, ...). Read-only, never emits diagnostics — used
    /// to route `Member`/`MemberKey`/`Index` checking to the entity-NBT
    /// schema chain (PS-12, S-042) instead of ordinary struct/tuple
    /// checking. An executor capture is never an ordinary `ValueType`
    /// binding, so this can never misfire against a real struct/tuple value.
    fn expression_roots_in_nbt_path_receiver(&self, expression: &AstExpression) -> bool {
        match &expression.kind {
            AstExpressionKind::Name(name) => self.spelling(name.span).is_ok_and(|spelling| {
                self.active_executor_captures
                    .contains_key(spelling.as_ref())
            }),
            AstExpressionKind::Call(call) => self.call_is_block_ref_root_shape(call),
            AstExpressionKind::Member { receiver, .. }
            | AstExpressionKind::MemberKey { receiver, .. } => {
                self.expression_roots_in_nbt_path_receiver(receiver)
            }
            AstExpressionKind::Index { aggregate, .. } => {
                self.expression_roots_in_nbt_path_receiver(aggregate)
            }
            _ => false,
        }
    }

    /// Cheap, diagnostic-free shape check: does `call` look like `mc.block(
    /// ...)`, regardless of whether its arguments are actually valid? Real
    /// argument validation (kind name, literal integer positions) is
    /// `check_block_ref_root`'s job, run only once this gate has already
    /// decided the expression should be checked as an entity-NBT path.
    fn call_is_block_ref_root_shape(&self, call: &AstCall) -> bool {
        let AstExpressionKind::Member {
            receiver, member, ..
        } = &call.callee.kind
        else {
            return false;
        };
        let AstExpressionKind::Name(namespace) = receiver.kind else {
            return false;
        };
        self.spelling(namespace.span)
            .is_ok_and(|spelling| spelling.as_ref() == "mc")
            && self
                .spelling(member.span)
                .is_ok_and(|spelling| spelling.as_ref() == "block")
    }

    /// Checks the whole entity-NBT path chain rooted at `expression` and, on
    /// success, produces a checked value from its terminal `Scalar` schema
    /// node. Only call when `expression_roots_in_nbt_path_receiver` returned
    /// true. Reaching a non-terminal (`Compound`/`List`) node is rejected —
    /// binding an in-progress chain to an intermediate variable
    /// (`const item := reader.equipment.mainhand;`) is explicit deferred
    /// follow-up, not required by PS-12's exit criteria.
    fn check_entity_nbt_path_expression(
        &mut self,
        expression: &AstExpression,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        let Some(step) = self.check_entity_nbt_path_step(expression, assigned)? else {
            return Ok(CheckedExpression::invalid(expression.span));
        };
        let Some(result_ty) = step.node.scalar_type() else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    TYPE_MISMATCH,
                    "this entity-NBT path is not a complete field yet",
                    expression.span,
                )
                .primary(format!(
                    "chain reached {}, not a scalar field — continue with `.field`, \
                     `.\"resource:id\"`, or `[index]`",
                    step.node.kind_label()
                )),
            );
            return Ok(CheckedExpression::invalid(expression.span));
        };
        let id = SourceExternalOpId::from_index(self.external_ops.len()).ok_or(
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::ExternalOperation),
        )?;
        let origin = self.origin(expression.span)?;
        self.external_ops.push(HirExternalOp {
            id,
            semantic: HirExternalSemantic::EntityNbtRead {
                receiver: step.receiver,
                segments: step.segments.into_boxed_slice(),
                result_ty,
                receiver_origin: step.receiver_origin,
            },
            origin,
        });
        Ok(CheckedExpression::valid(
            HirExpressionKind::External(id),
            result_ty,
            origin,
            expression.span,
        ))
    }

    /// Validates a `mc.block(Kind, x, y, z)` call as an entity-NBT path root
    /// (BE-1). Only call once `expression_roots_in_nbt_path_receiver` has
    /// already recognized the shape via `call_is_block_ref_root_shape` — from
    /// here on, a shape mismatch is diagnosed, not silently ignored, mirroring
    /// `check_entity_query_root`'s own commitment discipline.
    fn check_block_ref_root(
        &mut self,
        call: &AstCall,
    ) -> Result<Option<(BlockEntityKind, BlockPosition)>, CheckError> {
        let [kind_arg, x_arg, y_arg, z_arg] = call.arguments.as_slice() else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INVALID_ENTITY_QUERY,
                    "`mc.block` requires one nominal block-entity kind and 3 absolute integer coordinates",
                    call.span,
                )
                .primary("expected `mc.block(Kind, x, y, z)`"),
            );
            return Ok(None);
        };
        let AstExpressionKind::Name(kind_name) = kind_arg.kind else {
            self.invalid_entity_query(
                kind_arg.span,
                "expected the nominal block-entity kind `Chest`",
            );
            return Ok(None);
        };
        let spelling = self.spelling(kind_name.span)?;
        let Some(kind) = BlockEntityKind::from_source_name(&spelling) else {
            self.invalid_entity_query(
                kind_name.span,
                format!("block-entity kind `{spelling}` is not supported by this slice"),
            );
            return Ok(None);
        };
        let Some(x) = self.check_block_axis(x_arg)? else {
            return Ok(None);
        };
        let Some(y) = self.check_block_axis(y_arg)? else {
            return Ok(None);
        };
        let Some(z) = self.check_block_axis(z_arg)? else {
            return Ok(None);
        };
        Ok(Some((kind, BlockPosition { x, y, z })))
    }

    /// Checks one `mc.block(...)` coordinate argument. Absolute integers
    /// only in this release (`block-entity-nbt-paths.md` §1.4/§2.2): a
    /// `~`/`^`-relative or fractional coordinate is a diagnosed rejection,
    /// not a silent truncation.
    fn check_block_axis(&mut self, expression: &AstExpression) -> Result<Option<i32>, CheckError> {
        let (spelling, negative) = match &expression.kind {
            AstExpressionKind::DecimalInteger(span) => (self.spelling(*span)?, false),
            AstExpressionKind::StaticDecimal {
                sigil: None,
                negative,
                digits: Some(span),
            } => (self.spelling(*span)?, *negative),
            AstExpressionKind::StaticDecimal { sigil: Some(_), .. } => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        LITERAL_CONTEXT_REQUIRED,
                        "block positions must be absolute in this release",
                        expression.span,
                    )
                    .primary("`~`/`^`-relative block coordinates are not supported yet"),
                );
                return Ok(None);
            }
            _ => {
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        LITERAL_CONTEXT_REQUIRED,
                        "expected a compiler-known absolute integer coordinate",
                        expression.span,
                    )
                    .primary("runtime expressions are not block-position coordinates yet"),
                );
                return Ok(None);
            }
        };
        if spelling.contains('.') {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    TYPE_MISMATCH,
                    "block coordinates must be whole integers",
                    expression.span,
                )
                .primary("this coordinate has a fractional part"),
            );
            return Ok(None);
        }
        let signed = if negative {
            format!("-{spelling}")
        } else {
            spelling.into()
        };
        if let Ok(value) = signed.parse::<i32>() {
            return Ok(Some(value));
        }
        self.diagnostics.push(
            PendingDiagnostic::new(
                INTEGER_OUT_OF_RANGE,
                "block coordinate is outside the Int32 range",
                expression.span,
            )
            .primary("expected a value from -2147483648 through 2147483647"),
        );
        Ok(None)
    }

    /// Recursive worker behind `check_entity_nbt_path_expression`. Returns
    /// `Ok(None)` once a diagnostic has already been pushed for a failure
    /// anywhere in the chain (unknown key, wrong step kind, stale capture,
    /// non-`Int32` or call-containing runtime index); the caller must not
    /// push a second diagnostic in that case.
    #[allow(
        clippy::too_many_lines,
        reason = "one exhaustive per-step-kind chain-resolution function keeps every failure diagnostic local to its cause"
    )]
    fn check_entity_nbt_path_step(
        &mut self,
        expression: &AstExpression,
        assigned: &Assigned,
    ) -> Result<Option<EntityPathStep>, CheckError> {
        match &expression.kind {
            AstExpressionKind::Name(name) => {
                let spelling = self.spelling(name.span)?;
                let Some(capture) = self
                    .active_executor_captures
                    .get(spelling.as_ref())
                    .copied()
                else {
                    return Ok(None);
                };
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
                                "executor capture `{spelling}` no longer proves the current executor"
                            ),
                            expression.span,
                        )
                        .primary("a nested executor transition replaced this exact proof")
                        .support(capture.name_span, "capture was established here"),
                    );
                    return Ok(None);
                }
                Ok(Some(EntityPathStep {
                    node: root_schema(capture.ty.kind()),
                    receiver: HirEntityNbtReceiver::Entity {
                        kind: capture.ty.kind(),
                        executor_proof: capture.proof,
                    },
                    receiver_origin: self.origin(expression.span)?,
                    segments: vec![],
                }))
            }
            AstExpressionKind::Call(call) => {
                let Some((kind, position)) = self.check_block_ref_root(call)? else {
                    return Ok(None);
                };
                Ok(Some(EntityPathStep {
                    node: block_root_schema(kind),
                    receiver: HirEntityNbtReceiver::Block { kind, position },
                    receiver_origin: self.origin(expression.span)?,
                    segments: vec![],
                }))
            }
            AstExpressionKind::Member {
                receiver, member, ..
            } => {
                let Some(mut step) = self.check_entity_nbt_path_step(receiver, assigned)? else {
                    return Ok(None);
                };
                let name = self.spelling(member.span)?;
                let Some(child) = step.node.field_by_name(name.as_ref()) else {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            UNKNOWN_MEMBER,
                            format!(
                                "unknown entity-NBT field `.{name}` on {}",
                                step.node.kind_label()
                            ),
                            member.span,
                        )
                        .primary("this key is not in the compiler-known schema"),
                    );
                    return Ok(None);
                };
                step.node = child;
                step.segments.push(HirEntityPathSegment::Key(name));
                Ok(Some(step))
            }
            AstExpressionKind::MemberKey { receiver, key, .. } => {
                let Some(mut step) = self.check_entity_nbt_path_step(receiver, assigned)? else {
                    return Ok(None);
                };
                let Some(content) = decode_string_literal(self.sources, *key)? else {
                    return Ok(None);
                };
                let Some(child) = step.node.field_by_resource_id(&content) else {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            UNKNOWN_MEMBER,
                            format!(
                                "unknown entity-NBT field .\"{content}\" on {}",
                                step.node.kind_label()
                            ),
                            *key,
                        )
                        .primary("this resource id is not in the compiler-known schema"),
                    );
                    return Ok(None);
                };
                step.node = child;
                step.segments.push(HirEntityPathSegment::Key(content));
                Ok(Some(step))
            }
            AstExpressionKind::Index { aggregate, index } => {
                let Some(mut step) = self.check_entity_nbt_path_step(aggregate, assigned)? else {
                    return Ok(None);
                };
                let match_key = step.node.match_key();
                let Some(element) = step.node.list_element() else {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            UNRESOLVED_MEMBER,
                            format!(
                                "index access requires a schema NBT-list receiver, found {}",
                                step.node.kind_label()
                            ),
                            expression.span,
                        )
                        .primary("only a known NBT-list field supports `[index]`"),
                    );
                    return Ok(None);
                };
                let checked_index = self.check_expression(index, assigned)?;
                let Some(index_expr) = checked_index.expression else {
                    return Ok(None);
                };
                if checked_index.ty != Some(ValueType::Int32) {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            TYPE_MISMATCH,
                            "entity-NBT list index must be an Int32",
                            index.span,
                        )
                        .primary("this index expression is not an Int32"),
                    );
                    return Ok(None);
                }
                if Self::runtime_index_contains_forbidden(&index_expr) {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            LITERAL_CONTEXT_REQUIRED,
                            "a runtime entity-NBT list index may not contain a function call yet",
                            index.span,
                        )
                        .primary("this expression contains a call or external operation"),
                    );
                    return Ok(None);
                }
                step.node = element;
                step.segments.push(match match_key {
                    Some(match_key) => HirEntityPathSegment::Match {
                        match_key: match_key.into(),
                        value: Box::new(index_expr),
                    },
                    None => HirEntityPathSegment::Index(Box::new(index_expr)),
                });
                Ok(Some(step))
            }
            _ => Ok(None),
        }
    }

    fn check_index_expression(
        &mut self,
        aggregate: &AstExpression,
        index: &AstExpression,
        expression_span: Span,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        let aggregate = self.check_expression(aggregate, assigned)?;
        let Some(ValueType::AnonymousStruct(struct_id)) = aggregate.ty else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNRESOLVED_MEMBER,
                    "index access requires a positional anonymous struct value",
                    expression_span,
                )
                .primary("only positional anonymous struct values support compile-time indexing"),
            );
            return Ok(CheckedExpression::invalid(expression_span));
        };
        let interner = self.signatures.anonymous.borrow();
        let anonymous = interner.entries.iter().find(|e| e.id == struct_id);
        let Some(anonymous) = anonymous else {
            return Ok(CheckedExpression::invalid(expression_span));
        };
        let AnonymousTypeKey::Positional(components) = &anonymous.key else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    UNRESOLVED_MEMBER,
                    "named anonymous structs use field projection, not index access",
                    expression_span,
                )
                .primary("named fields are accessed with `.field`, not `[index]`"),
            );
            return Ok(CheckedExpression::invalid(expression_span));
        };
        let AstExpressionKind::DecimalInteger(index_span) = index.kind else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INTEGER_OUT_OF_RANGE,
                    "index must be a non-negative integer literal",
                    index.span,
                )
                .primary("only compile-time integer literals are accepted as an index"),
            );
            return Ok(CheckedExpression::invalid(expression_span));
        };
        let index_text = self.spelling(index_span)?;
        let index: u32 = if let Ok(value) = index_text.parse() {
            value
        } else {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INTEGER_OUT_OF_RANGE,
                    "index must be a non-negative integer literal",
                    index_span,
                )
                .primary("only compile-time integer literals are accepted as an index"),
            );
            return Ok(CheckedExpression::invalid(expression_span));
        };
        let component = usize::try_from(index).ok();
        if component.is_none_or(|c| c >= components.len()) {
            self.diagnostics.push(
                PendingDiagnostic::new(
                    INTEGER_OUT_OF_RANGE,
                    format!(
                        "index {index} is out of bounds for anonymous struct with {} components",
                        components.len()
                    ),
                    index_span,
                )
                .primary("struct index is out of bounds"),
            );
            return Ok(CheckedExpression::invalid(expression_span));
        }
        let Some(aggregate) = aggregate.expression else {
            return Ok(CheckedExpression::invalid(expression_span));
        };
        Ok(CheckedExpression::valid(
            HirExpressionKind::Index {
                aggregate: Box::new(aggregate),
                component: index,
            },
            components[component.unwrap_or(usize::MAX)],
            self.origin(expression_span)?,
            expression_span,
        ))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "member, projection, and index-access checking keeps its case analysis in one exhaustive function"
    )]
    fn check_member_expression(
        &mut self,
        receiver: &AstExpression,
        member_span: Span,
        expression_span: Span,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        if let AstExpressionKind::Name(type_name) = receiver.kind {
            let module_index = self.signature.module.as_usize().unwrap_or(usize::MAX);
            let spelling = self.spelling(type_name.span)?;
            if let Some(TypeLookup::Enum { id: enum_id, .. }) = self
                .signatures
                .types_by_module
                .get(module_index)
                .and_then(|types| types.get(spelling.as_ref()))
                .copied()
            {
                return self.check_enum_literal(
                    Some(type_name),
                    AstName { span: member_span },
                    Some(ValueType::Enum(enum_id)),
                    expression_span,
                );
            }
        }
        let receiver = self.check_expression(receiver, assigned)?;
        match receiver.ty {
            Some(ValueType::Struct(struct_id)) => {
                let Some(struct_) = struct_id
                    .as_usize()
                    .and_then(|index| self.signatures.structs.get(index))
                else {
                    return Ok(CheckedExpression::invalid(expression_span));
                };
                let member = self.spelling(member_span)?;
                let Some((index, field)) = struct_
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, field)| field.name.as_ref() == member.as_ref())
                else {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            UNKNOWN_STRUCT_FIELD,
                            format!("struct has no field `{member}`"),
                            member_span,
                        )
                        .primary("unknown struct field"),
                    );
                    return Ok(CheckedExpression::invalid(expression_span));
                };
                let Some(receiver) = receiver.expression else {
                    return Ok(CheckedExpression::invalid(expression_span));
                };
                Ok(CheckedExpression::valid(
                    HirExpressionKind::StructProject {
                        aggregate: Box::new(receiver),
                        field: u32::try_from(index).unwrap_or(u32::MAX),
                    },
                    field.ty,
                    self.origin(expression_span)?,
                    expression_span,
                ))
            }
            Some(ValueType::AnonymousStruct(struct_id)) => {
                let interner = self.signatures.anonymous.borrow();
                let anonymous = interner.entries.iter().find(|e| e.id == struct_id);
                let Some(anonymous) = anonymous else {
                    return Ok(CheckedExpression::invalid(expression_span));
                };
                let fields: &[(Box<str>, ValueType)] = match &anonymous.key {
                    AnonymousTypeKey::Named(fields) => fields,
                    AnonymousTypeKey::Positional { .. } => {
                        let member = self.spelling(member_span)?;
                        self.diagnostics.push(
                            PendingDiagnostic::new(
                                UNRESOLVED_MEMBER,
                                format!("member `{member}` cannot be resolved on a positional anonymous struct"),
                                member_span,
                            )
                            .primary("positional anonymous structs use index access, not field projection"),
                        );
                        return Ok(CheckedExpression::invalid(expression_span));
                    }
                };
                let member = self.spelling(member_span)?;
                let Some((index, field)) = fields
                    .iter()
                    .enumerate()
                    .find(|(_, (name, _))| name.as_ref() == member.as_ref())
                else {
                    self.diagnostics.push(
                        PendingDiagnostic::new(
                            UNKNOWN_STRUCT_FIELD,
                            format!("anonymous struct has no field `{member}`"),
                            member_span,
                        )
                        .primary("unknown field"),
                    );
                    return Ok(CheckedExpression::invalid(expression_span));
                };
                let Some(receiver) = receiver.expression else {
                    return Ok(CheckedExpression::invalid(expression_span));
                };
                Ok(CheckedExpression::valid(
                    HirExpressionKind::AnonymousStructProject {
                        aggregate: Box::new(receiver),
                        field: u32::try_from(index).unwrap_or(u32::MAX),
                    },
                    field.1,
                    self.origin(expression_span)?,
                    expression_span,
                ))
            }
            _ => {
                let member = self.spelling(member_span)?;
                self.diagnostics.push(
                    PendingDiagnostic::new(
                        UNRESOLVED_MEMBER,
                        format!("member `{member}` cannot be resolved on this value"),
                        member_span,
                    )
                    .primary("field access requires a struct value"),
                );
                Ok(CheckedExpression::invalid(expression_span))
            }
        }
    }

    fn check_wrapping_arithmetic(
        &mut self,
        op: AstWrappingArithmeticOp,
        left: &AstExpression,
        right: &AstExpression,
        span: Span,
        assigned: &Assigned,
    ) -> Result<CheckedExpression, CheckError> {
        let left = self.check_expression(left, assigned)?;
        let right = self.check_expression(right, assigned)?;
        let mut valid = left.expression.is_some() && right.expression.is_some();
        for operand in [&left, &right] {
            if let Some(actual) = operand.ty {
                if actual != ValueType::Int32 {
                    self.type_mismatch_without_support(
                        operand.span,
                        actual,
                        ValueType::Int32,
                        "wrapping arithmetic requires Int32 operands",
                    );
                    valid = false;
                }
            } else {
                valid = false;
            }
        }
        let kind = match (valid, left.expression, right.expression) {
            (true, Some(left), Some(right)) => HirExpressionKind::WrappingArithmetic {
                op: match op {
                    AstWrappingArithmeticOp::Add => HirWrappingArithmeticOp::Add,
                    AstWrappingArithmeticOp::Subtract => HirWrappingArithmeticOp::Subtract,
                },
                left: Box::new(left),
                right: Box::new(right),
            },
            _ => return Ok(CheckedExpression::invalid(span)),
        };
        Ok(CheckedExpression::valid(
            kind,
            ValueType::Int32,
            self.origin(span)?,
            span,
        ))
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
        let (left, right) = if matches!(left.kind, AstExpressionKind::InferredEnumLiteral(_)) {
            let right = self.check_expression(right, assigned)?;
            let left = self.check_expression_expected(left, assigned, right.ty)?;
            (left, right)
        } else {
            let left = self.check_expression(left, assigned)?;
            let right = self.check_expression_expected(right, assigned, left.ty)?;
            (left, right)
        };
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
            let checked = self.check_expression_expected(
                argument,
                assigned,
                expected.map(|expected| expected.ty),
            )?;
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
        if require_value && descriptor.signature().results().is_empty() {
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
            result: Some(if descriptor.signature().results().is_empty() {
                FunctionResult::Void
            } else {
                FunctionResult::Value(ValueType::String)
            }),
        }))
    }

    fn runtime_index_contains_forbidden(expression: &HirExpression) -> bool {
        match &expression.kind {
            HirExpressionKind::Call(_) | HirExpressionKind::External(_) => true,
            HirExpressionKind::StructConstruct { fields, .. }
            | HirExpressionKind::AnonymousStructConstruct { fields, .. } => fields
                .iter()
                .any(|field| Self::runtime_index_contains_forbidden(&field.value)),
            HirExpressionKind::StructProject { aggregate, .. }
            | HirExpressionKind::AnonymousStructProject { aggregate, .. }
            | HirExpressionKind::Index { aggregate, .. }
            | HirExpressionKind::Not(aggregate) => {
                Self::runtime_index_contains_forbidden(aggregate)
            }
            HirExpressionKind::ListI32 { operands, .. }
            | HirExpressionKind::String { operands, .. } => {
                operands.iter().any(Self::runtime_index_contains_forbidden)
            }
            HirExpressionKind::WrappingArithmetic { left, right, .. }
            | HirExpressionKind::Compare { left, right, .. } => {
                Self::runtime_index_contains_forbidden(left)
                    || Self::runtime_index_contains_forbidden(right)
            }
            HirExpressionKind::Switch(switch) => {
                Self::runtime_index_contains_forbidden(&switch.scrutinee)
                    || switch
                        .arms
                        .iter()
                        .any(|arm| Self::runtime_index_contains_forbidden(&arm.body))
            }
            HirExpressionKind::Bool(_)
            | HirExpressionKind::Int32(_)
            | HirExpressionKind::EnumVariant { .. }
            | HirExpressionKind::Local(_) => false,
        }
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

/// Accumulated state of one in-progress entity-NBT path chain (PS-12, S-042).
/// `node` is the schema position the chain has narrowed to so far; `segments`
/// is the HIR path built alongside it.
struct EntityPathStep {
    node: SchemaNode,
    receiver: HirEntityNbtReceiver,
    receiver_origin: crate::source::OriginId,
    segments: Vec<HirEntityPathSegment>,
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

struct SwitchCoverage {
    ty: ValueType,
    enum_seen: Vec<Option<Span>>,
    intervals: BTreeMap<i32, (i32, Span)>,
    else_span: Option<Span>,
}

impl SwitchCoverage {
    fn new(ty: ValueType, variant_count: usize) -> Self {
        Self {
            ty,
            enum_seen: vec![None; variant_count],
            intervals: BTreeMap::new(),
            else_span: None,
        }
    }

    fn insert_variant(&mut self, index: usize, span: Span) -> Option<Span> {
        let slot = self.enum_seen.get_mut(index)?;
        if let Some(previous) = *slot {
            Some(previous)
        } else {
            *slot = Some(span);
            None
        }
    }

    fn insert_interval(&mut self, min: i32, max: i32, span: Span) -> Option<Span> {
        if let Some((_, (previous_max, previous_span))) = self.intervals.range(..=min).next_back() {
            if *previous_max >= min {
                return Some(*previous_span);
            }
        }
        if let Some((next_min, (_, next_span))) = self.intervals.range(min..).next() {
            if *next_min <= max {
                return Some(*next_span);
            }
        }
        self.intervals.insert(min, (max, span));
        None
    }

    fn is_complete(&self) -> bool {
        match self.ty {
            ValueType::Enum(_) => {
                !self.enum_seen.is_empty() && self.enum_seen.iter().all(Option::is_some)
            }
            ValueType::Int32 => {
                let mut expected = i64::from(i32::MIN);
                for (min, (max, _)) in &self.intervals {
                    if i64::from(*min) != expected {
                        return false;
                    }
                    expected = i64::from(*max) + 1;
                }
                expected == i64::from(i32::MAX) + 1
            }
            _ => false,
        }
    }
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
        ARGUMENT_COUNT, CheckOutput, DIRTY_AST, DUPLICATE_BINDING, DUPLICATE_EVENT_ARGUMENT,
        DUPLICATE_FUNCTION, ENUM_CONTEXT_REQUIRED, ENUM_EXPORT_ABI, IMMUTABLE_ASSIGNMENT,
        INTEGER_OUT_OF_RANGE, INVALID_ENTITY_TAG, INVALID_EVENT_ITEM, INVALID_EXECUTOR_CAPTURE,
        INVALID_MESSAGE_LITERAL, INVALID_MINECRAFT_METHOD_RECEIVER, INVALID_SWITCH_ELSE,
        INVALID_SWITCH_PATTERN, INVALID_UNSAFE_COMMAND, LITERAL_CONTEXT_REQUIRED,
        MESSAGE_LITERAL_REQUIRED, MISSING_EVENT_ARGUMENT, MISSING_RETURN, NON_EXHAUSTIVE_SWITCH,
        OVERLAPPING_SWITCH_PATTERN, RESERVED_COMPILER_NAME, RETURN_IN_RUN_SCOPE,
        RETURN_VALUE_FORBIDDEN, RETURN_VALUE_REQUIRED, SCOPED_CAPABILITY_VALUE, TRUNCATED,
        TYPE_MISMATCH, UNINITIALIZED_READ, UNKNOWN_EVENT_ARGUMENT, UNKNOWN_EVENT_TRIGGER,
        UNKNOWN_MEMBER, UNKNOWN_NAME, UNRESOLVED_MEMBER, UNSUPPORTED_RUN_SCALAR_CAPTURE,
        VOID_VALUE, check,
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
    fn event_handler_seeds_player_as_an_established_executor_capture() {
        let source = r#"on inventory_changed(.items = ["minecraft:diamond"]) |player| {
    player.say("MDL_GOT_DIAMOND");
}"#;
        let (sources, _, output) = check_text(source);
        assert_eq!(output.diagnostics(), None);
        let checked = output.checked().unwrap();
        assert_eq!(checked.function_count(), 1);
        assert_eq!(checked.event_handlers().len(), 1);
        let dump = checked.dump(&sources);
        assert!(dump.contains("entry-capture Executor<Player>"));
        assert!(dump.contains(
            "on reward=@0 inventory_changed items=[\"minecraft:diamond\"] items_origin="
        ));
        assert!(dump.contains("minecraft.Say receiver=Executor<Player>"));
        assert!(dump.contains("message=\"MDL_GOT_DIAMOND\""));
    }

    #[test]
    fn event_handler_rejects_an_unknown_trigger_name() {
        let (_, _, output) =
            check_text(r#"on nonexistent_trigger(.items = ["minecraft:diamond"]) |player| { }"#);
        assert_eq!(codes(&output), [UNKNOWN_EVENT_TRIGGER]);
    }

    #[test]
    fn event_handler_rejects_an_unknown_trigger_argument_name() {
        let (_, _, output) =
            check_text(r#"on inventory_changed(.count = ["minecraft:diamond"]) |player| { }"#);
        assert_eq!(
            codes(&output),
            [UNKNOWN_EVENT_ARGUMENT, MISSING_EVENT_ARGUMENT]
        );
    }

    #[test]
    fn event_handler_requires_exactly_one_items_argument() {
        let (_, _, missing) = check_text(r"on inventory_changed() |player| { }");
        assert_eq!(codes(&missing), [MISSING_EVENT_ARGUMENT]);

        let (_, _, duplicate) = check_text(
            r#"on inventory_changed(.items = ["minecraft:diamond"], .items = ["minecraft:emerald"]) |player| { }"#,
        );
        assert_eq!(codes(&duplicate), [DUPLICATE_EVENT_ARGUMENT]);
    }

    #[test]
    fn event_handler_rejects_malformed_or_empty_item_ids() {
        let (_, _, empty_list) = check_text(r"on inventory_changed(.items = []) |player| { }");
        assert_eq!(codes(&empty_list), [MISSING_EVENT_ARGUMENT]);

        let (_, _, bad_spelling) =
            check_text(r#"on inventory_changed(.items = ["diamond"]) |player| { }"#);
        assert_eq!(
            codes(&bad_spelling),
            [INVALID_EVENT_ITEM, MISSING_EVENT_ARGUMENT]
        );

        let (_, _, one_bad_one_good) = check_text(
            r#"on inventory_changed(.items = ["minecraft:diamond", "nope"]) |player| { }"#,
        );
        assert_eq!(codes(&one_bad_one_good), [INVALID_EVENT_ITEM]);
    }

    #[test]
    fn event_handler_binding_follows_the_same_reserved_and_duplicate_name_rules() {
        let (_, _, reserved) =
            check_text(r#"on inventory_changed(.items = ["minecraft:diamond"]) |mc| { }"#);
        assert_eq!(codes(&reserved), [RESERVED_COMPILER_NAME]);
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

    #[test]
    fn enums_are_nominal_contextual_and_usable_in_structs_calls_and_returns() {
        let text = r"
            const A = enum { one, two };
            const B = enum { one, two };
            const Boxed = struct { value: A };
            fn id(value: A) -> A { return value; }
            fn okay() -> Bool {
                const value: A = .one;
                const boxed: Boxed = Boxed{ .value = id(value) };
                return boxed.value == A.one;
            }
            fn wrong() { const value: A = B.one; }
            fn context_free() { const value: Bool = .one == .one; }
        ";
        let (_, _, output) = check_text(text);
        assert!(codes(&output).contains(&TYPE_MISMATCH));
        assert!(codes(&output).contains(&ENUM_CONTEXT_REQUIRED));
    }

    #[test]
    fn enum_values_flow_through_branches_loops_and_private_recursion() {
        let text = r"
            const State = enum { first, second };
            fn flip(state: State) -> State {
                return switch (state) { .first => .second, .second => .first };
            }
            fn recurse(state: State, fuel: Int32) -> State {
                if (fuel == 0) { return state; }
                return recurse(flip(state), fuel -% 1);
            }
            fn iterate(state: State) -> State {
                var current: State = state;
                var index: Int32 = 0;
                while (index < 2) {
                    current = flip(current);
                    index = index +% 1;
                }
                return current;
            }
            fn valid() -> Bool { return recurse(.first, 2) == iterate(.first); }
        ";
        let (_, _, output) = check_text(text);
        assert_eq!(output.diagnostics(), None);
        assert!(output.checked().is_some());
    }

    #[test]
    fn switch_coverage_rejects_overlap_holes_bad_ranges_and_unreachable_else() {
        let text = r"
            const State = enum { a, b, c };
            fn overlap(value: Int32) -> Int32 { return switch (value) { 0...10 => 1, 10...20 => 2, else => 3 }; }
            fn hole(state: State) -> Int32 { return switch (state) { .a => 1, .b => 2 }; }
            fn backwards(value: Int32) -> Int32 { return switch (value) { 3...1 => 1, else => 2 }; }
            fn unreachable(state: State) -> Int32 { return switch (state) { .a => 1, .b => 2, .c => 3, else => 4 }; }
        ";
        let (_, _, output) = check_text(text);
        let codes = codes(&output);
        assert!(codes.contains(&OVERLAPPING_SWITCH_PATTERN));
        assert!(codes.contains(&NON_EXHAUSTIVE_SWITCH));
        assert!(codes.contains(&INVALID_SWITCH_PATTERN));
        assert!(codes.contains(&INVALID_SWITCH_ELSE));
    }

    #[test]
    fn datapack_exports_reject_enum_abi_directly_and_through_structs() {
        let text = r"
            const State = enum { ready };
            const Boxed = struct { state: State };
            export fn direct(value: State) -> Int32 { return 0; }
            export fn nested(value: Boxed) -> Boxed { return value; }
        ";
        let (_, _, output) = check_text(text);
        assert_eq!(
            codes(&output)
                .iter()
                .filter(|code| **code == ENUM_EXPORT_ABI)
                .count(),
            3
        );
    }

    #[test]
    fn hundreds_of_descending_switch_intervals_use_ordered_coverage_state() {
        use std::fmt::Write as _;

        let mut text = String::from("fn classify(value: Int32) -> Int32 { return switch (value) {");
        for value in (0..512).rev() {
            write!(text, "{value} => {value},").unwrap();
        }
        text.push_str("else => 0, }; }");
        let (_, _, output) = check_text(&text);
        assert_eq!(output.diagnostics(), None);
        assert!(output.checked().is_some());
    }

    // PS-12B: schema table + unified chained-postfix checker for entity-NBT
    // paths (S-042). See notes/compiler/entity-nbt-path-composability.md §2.3.

    #[test]
    fn full_entity_nbt_chain_to_raw_type_checks_with_a_literal_index() {
        let source = r#"fn read() {
            run.as(mc.entities(ArmorStand).with_tag("x").limit(1)) |reader| {
                const page: String = reader.equipment.mainhand.components."minecraft:written_book_content".pages[0].raw;
            }
        }"#;
        let (sources, _, output) = check_text(source);
        assert_eq!(output.diagnostics(), None);
        let checked = output.checked().unwrap();
        assert_eq!(checked.external_operation_count(), 1);
        let dump = checked.dump(&sources);
        assert!(
            dump.contains("entity-nbt-read receiver=Executor<ArmorStand>"),
            "{dump}"
        );
        assert!(
            dump.contains(
                "path=.equipment.mainhand.components.minecraft:written_book_content.pages[0].raw"
            ),
            "{dump}"
        );
        assert!(dump.contains("result_ty=String"), "{dump}");
    }

    #[test]
    fn full_entity_nbt_chain_to_raw_type_checks_with_a_genuinely_runtime_index() {
        let source = r#"fn read() {
            run.as(mc.entities(ArmorStand).limit(1)) |reader| {
                const index: Int32 = 0;
                const page: String = reader.equipment.offhand.components."minecraft:written_book_content".pages[index].raw;
            }
        }"#;
        let (sources, _, output) = check_text(source);
        assert_eq!(output.diagnostics(), None);
        let checked = output.checked().unwrap();
        let dump = checked.dump(&sources);
        assert!(
            dump.contains(
                "path=.equipment.offhand.components.minecraft:written_book_content.pages[<runtime>].raw"
            ),
            "{dump}"
        );
    }

    #[test]
    fn every_terminal_schema_field_and_every_equipment_slot_is_reachable() {
        for slot in ["mainhand", "offhand", "head", "chest", "legs", "feet"] {
            let source = format!(
                r#"fn read() {{
                    run.as(mc.entities(ArmorStand).limit(1)) |reader| {{
                        const author: String = reader.equipment.{slot}.components."minecraft:written_book_content".author;
                        const resolved: Bool = reader.equipment.{slot}.components."minecraft:written_book_content".resolved;
                        const title: String = reader.equipment.{slot}.components."minecraft:written_book_content".title.raw;
                    }}
                }}"#
            );
            let (_, _, output) = check_text(&source);
            assert_eq!(output.diagnostics(), None, "slot {slot}");
            assert_eq!(output.checked().unwrap().external_operation_count(), 3);
        }
    }

    #[test]
    fn unknown_key_is_rejected_at_every_chain_depth() {
        let cases = [
            "reader.nonexistent",
            "reader.equipment.nonexistent",
            "reader.equipment.mainhand.nonexistent",
            "reader.equipment.mainhand.components.\"minecraft:unknown_component\"",
            "reader.equipment.mainhand.components.\"minecraft:written_book_content\".nonexistent",
        ];
        for chain in cases {
            let source = format!(
                r"fn bad() {{
                    run.as(mc.entities(ArmorStand).limit(1)) |reader| {{
                        const x := {chain};
                    }}
                }}"
            );
            let (_, _, output) = check_text(&source);
            assert_eq!(codes(&output), [UNKNOWN_MEMBER], "{chain}");
        }
    }

    #[test]
    fn nbt_list_index_syntax_is_rejected_on_a_non_list_schema_node() {
        let source = r"fn bad() {
            run.as(mc.entities(ArmorStand).limit(1)) |reader| {
                const x := reader.equipment[0];
            }
        }";
        let (_, _, output) = check_text(source);
        assert_eq!(codes(&output), [UNRESOLVED_MEMBER]);
    }

    #[test]
    fn ordinary_positional_tuple_indexing_still_requires_a_compile_time_literal() {
        // Disambiguation proof: a receiver that is NOT rooted in an executor
        // capture is entirely unaffected by the entity-NBT schema checker —
        // the pre-existing PS-5 literal-only rule for anonymous-struct tuple
        // indexing is untouched.
        let source = "fn bad(pair: { Int32, Int32 }, i: Int32) -> Int32 { return pair[i]; }";
        let (_, _, output) = check_text(source);
        assert_eq!(codes(&output), [INTEGER_OUT_OF_RANGE]);
    }

    #[test]
    fn an_intermediate_non_scalar_chain_position_is_rejected() {
        // Binding a non-terminal schema position to a variable
        // (`const item := reader.equipment.mainhand;` then `item.components...`)
        // is explicit deferred follow-up, not part of PS-12's exit criteria —
        // this proves it fails with a clear diagnostic rather than silently
        // misbehaving or panicking.
        let source = r"fn bad() {
            run.as(mc.entities(ArmorStand).limit(1)) |reader| {
                const item := reader.equipment.mainhand;
            }
        }";
        let (_, _, output) = check_text(source);
        assert_eq!(codes(&output), [TYPE_MISMATCH]);
    }

    #[test]
    fn a_stale_executor_capture_is_rejected_through_the_entity_nbt_chain() {
        let source = r"fn bad() {
            run.as(mc.entities(ArmorStand).limit(1)) |outer| {
                run.as(mc.entities(ArmorStand).limit(1)) |inner| {
                    const x := outer.equipment;
                }
            }
        }";
        let (_, _, output) = check_text(source);
        assert_eq!(codes(&output), [SCOPED_CAPABILITY_VALUE]);
    }

    #[test]
    fn a_runtime_list_index_containing_a_call_is_rejected() {
        let source = r#"fn helper() -> Int32 { return 0; }
        fn bad() {
            run.as(mc.entities(ArmorStand).limit(1)) |reader| {
                const x := reader.equipment.mainhand.components."minecraft:written_book_content".pages[helper()].raw;
            }
        }"#;
        let (_, _, output) = check_text(source);
        assert_eq!(codes(&output), [LITERAL_CONTEXT_REQUIRED]);
    }

    #[test]
    fn a_non_int32_list_index_is_rejected() {
        let source = r#"fn bad() {
            run.as(mc.entities(ArmorStand).limit(1)) |reader| {
                const x := reader.equipment.mainhand.components."minecraft:written_book_content".pages[true].raw;
            }
        }"#;
        let (_, _, output) = check_text(source);
        assert_eq!(codes(&output), [TYPE_MISMATCH]);
    }
}
