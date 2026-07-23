//! Immutable resolved and fully typed frontend IR.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;

use super::context::{apply_run_modifiers, function_entry_context};
use super::input::ModuleKey;
use crate::ir::command_line::validate_command_line_shape;
use crate::ir::semantic::{
    Axes, BlockEntityKind, BlockPosition, DimensionKey, EntityAnchor, EntityCapability, EntityKind,
    EntityTag, ExecutionContext, ExecutorType, FunctionBehavior, MessageLiteral,
    MinecraftSemanticKey, PositionSpec, RelativeWorldOffset, RotationSpec, StaticEntityQuery,
    minecraft_descriptor,
};
use crate::source::{OriginId, SourceContext};

/// One runtime value type retained by typed source HIR.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ValueType {
    Bool,
    Int32,
    ListI32,
    String,
    Struct(SourceStructId),
    Enum(SourceEnumId),
    AnonymousStruct(SourceAnonymousStructId),
}

impl fmt::Display for ValueType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool => formatter.write_str("Bool"),
            Self::Int32 => formatter.write_str("Int32"),
            Self::ListI32 => formatter.write_str("List<Int32>"),
            Self::String => formatter.write_str("String"),
            Self::Struct(id) => write!(formatter, "Struct@{}", id.index()),
            Self::Enum(id) => write!(formatter, "Enum@{}", id.index()),
            Self::AnonymousStruct(id) => write!(formatter, "AnonStruct@{}", id.index()),
        }
    }
}

/// Dense canonical identity of one structural anonymous struct type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceAnonymousStructId(u32);

impl SourceAnonymousStructId {
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

/// Dense identity of one nominal enum in canonical package order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceEnumId(u32);

impl SourceEnumId {
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

/// Dense identity of one variant within its owning enum.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceVariantId(u32);

impl SourceVariantId {
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

/// Dense identity of one nominal struct in canonical package order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceStructId(u32);

impl SourceStructId {
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

/// Dense identity of one source module in canonical package order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceModuleId(u32);

impl SourceModuleId {
    /// Returns this identity's zero-based canonical package index.
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

/// Dense identity of one source external operation in canonical source order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceExternalOpId(u32);

impl SourceExternalOpId {
    /// Returns this identity's zero-based source-order index.
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

/// Dense identity of one structured run scope in canonical source order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceRunId(u32);

impl SourceRunId {
    /// Returns this identity's zero-based source-order index.
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

/// Source visibility and datapack-entry linkage of one checked function.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FunctionVisibility {
    /// Visible only inside the declaring source module.
    Private,
    /// Callable through an importing package module but not a datapack entry.
    Public,
    /// Published as a supported generated datapack entry.
    DatapackExport,
}

impl fmt::Display for FunctionVisibility {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Private => formatter.write_str("private"),
            Self::Public => formatter.write_str("public"),
            Self::DatapackExport => formatter.write_str("export"),
        }
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

    #[must_use]
    pub fn struct_count(&self) -> usize {
        self.module.structs.len()
    }

    #[must_use]
    pub fn struct_ids(&self) -> impl ExactSizeIterator<Item = SourceStructId> + '_ {
        self.module.structs.iter().map(|struct_| struct_.id)
    }

    #[must_use]
    pub fn enum_count(&self) -> usize {
        self.module.enums.len()
    }

    #[must_use]
    pub fn enum_ids(&self) -> impl ExactSizeIterator<Item = SourceEnumId> + '_ {
        self.module.enums.iter().map(|enum_| enum_.id)
    }

    #[must_use]
    pub fn anonymous_struct_count(&self) -> usize {
        self.module.anonymous_structs.len()
    }

    /// Returns whether the checked module contains no functions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.module.functions.is_empty()
    }

    /// Returns the number of modules in canonical package order.
    #[must_use]
    pub fn module_count(&self) -> usize {
        self.module.modules.len()
    }

    /// Iterates source module IDs in deterministic canonical package order.
    #[must_use]
    pub fn module_ids(&self) -> impl ExactSizeIterator<Item = SourceModuleId> + '_ {
        self.module.modules.iter().map(|module| module.id)
    }

    /// Returns the number of closed external operations in canonical source order.
    #[must_use]
    pub fn external_operation_count(&self) -> usize {
        self.module.external_ops.len()
    }

    /// Iterates external-operation identities in deterministic canonical source order.
    #[must_use]
    pub fn external_operation_ids(&self) -> impl ExactSizeIterator<Item = SourceExternalOpId> + '_ {
        self.module.external_ops.iter().map(|external| external.id)
    }

    /// Returns the number of structured run scopes in canonical source order.
    #[must_use]
    pub const fn run_scope_count(&self) -> usize {
        self.module.run_scope_count
    }

    /// Iterates dense structured-run identities in canonical source order.
    ///
    /// # Panics
    ///
    /// Only if a previously verified HIR inventory exceeds its `u32` identity domain.
    #[must_use]
    pub fn run_scope_ids(&self) -> impl ExactSizeIterator<Item = SourceRunId> {
        (0..self.module.run_scope_count).map(|index| {
            SourceRunId::from_index(index).expect("verified run count fits identity space")
        })
    }

    /// Returns the distinguished root module.
    #[must_use]
    pub const fn root_module(&self) -> SourceModuleId {
        self.module.root
    }

    /// Returns one module's driver-owned identity.
    #[must_use]
    pub fn module_key(&self, module: SourceModuleId) -> Option<&ModuleKey> {
        self.module
            .modules
            .get(module.as_usize()?)
            .map(|module| &module.key)
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

    /// Returns the module that owns a function.
    #[must_use]
    pub fn function_module(&self, function: SourceFunctionId) -> Option<SourceModuleId> {
        self.function(function).map(|function| function.module)
    }

    /// Returns a function's source visibility and datapack-entry intent.
    #[must_use]
    pub fn function_visibility(&self, function: SourceFunctionId) -> Option<FunctionVisibility> {
        self.function(function).map(|function| function.visibility)
    }

    /// Returns a source function's verified, transitively inferred behavior.
    #[must_use]
    pub fn function_behavior(&self, function: SourceFunctionId) -> Option<FunctionBehavior> {
        self.module.behaviors.get(function.as_usize()?).copied()
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

    pub(super) fn event_handlers(&self) -> &[HirEventHandler] {
        &self.module.event_handlers
    }

    pub(super) fn structs(&self) -> &[HirStruct] {
        &self.module.structs
    }

    pub(super) fn enums(&self) -> &[HirEnum] {
        &self.module.enums
    }
    pub(super) fn enum_(&self, id: SourceEnumId) -> Option<&HirEnum> {
        self.module.enums.get(id.as_usize()?)
    }
    pub(super) fn anonymous_struct(
        &self,
        id: SourceAnonymousStructId,
    ) -> Option<&HirAnonymousStruct> {
        self.module.anonymous_structs.get(id.as_usize()?)
    }

    pub(super) fn struct_(&self, id: SourceStructId) -> Option<&HirStruct> {
        self.module.structs.get(id.as_usize()?)
    }

    pub(super) fn external_ops(&self) -> &[HirExternalOp] {
        &self.module.external_ops
    }

    pub(super) fn external_op(&self, id: SourceExternalOpId) -> Option<&HirExternalOp> {
        self.module.external_ops.get(id.as_usize()?)
    }

    fn function(&self, id: SourceFunctionId) -> Option<&HirFunction> {
        self.module.functions.get(id.as_usize()?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirModule {
    pub(super) root: SourceModuleId,
    pub(super) modules: Box<[HirModuleInfo]>,
    pub(super) structs: Box<[HirStruct]>,
    pub(super) enums: Box<[HirEnum]>,
    pub(super) anonymous_structs: Box<[HirAnonymousStruct]>,
    pub(super) external_ops: Box<[HirExternalOp]>,
    pub(super) run_scope_count: usize,
    pub(super) functions: Box<[HirFunction]>,
    pub(super) behaviors: Box<[FunctionBehavior]>,
    pub(super) event_handlers: Box<[HirEventHandler]>,
    pub(super) origin: OriginId,
}

/// One push-model event-handler declaration (PS-15): `criterion` is the
/// vanilla advancement trigger this handler installs, `reward` names the
/// checked zero-parameter `Void` [`HirFunction`] (already present in the
/// same dense `functions` inventory as every ordinary function) that runs
/// when the criterion is satisfied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirEventHandler {
    pub(super) reward: SourceFunctionId,
    pub(super) criterion: HirCriterion,
    pub(super) origin: OriginId,
}

/// Closed trigger vocabulary (Slice 1: one variant). Growing this to a
/// second trigger is "add a variant", not a redesign.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirCriterion {
    InventoryChanged {
        /// Validated `namespace:path` item-resource-id spellings.
        items: Box<[Box<str>]>,
        items_origin: OriginId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirAnonymousStruct {
    pub(super) id: SourceAnonymousStructId,
    pub(super) kind: HirAnonymousStructKind,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirAnonymousStructKind {
    Named(Box<[HirAnonymousStructField]>),
    Positional(Box<[ValueType]>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirAnonymousStructField {
    pub(super) name: Box<str>,
    pub(super) ty: ValueType,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirEnum {
    pub(super) id: SourceEnumId,
    pub(super) module: SourceModuleId,
    pub(super) name_origin: OriginId,
    pub(super) variants: Box<[HirEnumVariant]>,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirEnumVariant {
    pub(super) id: SourceVariantId,
    pub(super) name_origin: OriginId,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirStruct {
    pub(super) id: SourceStructId,
    pub(super) module: SourceModuleId,
    pub(super) name_origin: OriginId,
    pub(super) fields: Box<[HirStructField]>,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirStructField {
    pub(super) name_origin: OriginId,
    pub(super) ty: ValueType,
    pub(super) type_origin: OriginId,
    pub(super) origin: OriginId,
}

/// One closed source external operation with semantic identity and source data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirExternalOp {
    pub(super) id: SourceExternalOpId,
    pub(super) semantic: HirExternalSemantic,
    pub(super) origin: OriginId,
}

/// External meanings admitted by the current HIR boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirExternalSemantic {
    UnsafeMinecraftCommand {
        command: Box<str>,
        command_origin: OriginId,
    },
    MinecraftOperation {
        key: MinecraftSemanticKey,
        receiver_kind: EntityKind,
        executor_proof: HirContextStep,
        attributes: HirMinecraftOperationAttributes,
        call_origin: OriginId,
        member_origin: OriginId,
        receiver_origin: OriginId,
    },
    /// A schema-typed entity-NBT path read (PS-12, S-042), e.g.
    /// `reader.equipment.mainhand.components."minecraft:written_book_content"
    /// .pages[index].raw`. Never routed through `MinecraftSemanticKey` /
    /// `MinecraftOperationAttributes` — the schema table
    /// (`entity_schema.rs`) is the only source of path-step validity, and
    /// version-dependence belongs to schema table selection, not a recipe.
    EntityNbtRead {
        receiver: HirEntityNbtReceiver,
        segments: Box<[HirEntityPathSegment]>,
        result_ty: ValueType,
        receiver_origin: OriginId,
    },
    /// A whole-slot entity-NBT write (PS-16, BE-2), e.g.
    /// `mc.block(Chest, x, y, z).Items[slot] = .{.id = "...", .count = N}`.
    /// `segments` ends in the `Match`/`Index` step selecting the written
    /// element (mirrors `EntityNbtRead`'s own path, reused unchanged).
    /// `item_id`/`count` may each be a compile-time literal or a runtime
    /// expression (checked `String`/`Int32` respectively) — never routed
    /// through `MinecraftSemanticKey` either, for the same schema-driven
    /// reason `EntityNbtRead` isn't.
    EntityNbtWrite {
        receiver: HirEntityNbtReceiver,
        segments: Box<[HirEntityPathSegment]>,
        item_id: Box<HirExpression>,
        count: Box<HirExpression>,
        receiver_origin: OriginId,
    },
}

/// The root of a checked entity-NBT path chain (BE-1,
/// `block-entity-nbt-paths.md` §2.3). An entity receiver still carries only
/// its kind — the real selector is resolved from ambient executor context at
/// lowering time, unchanged from PS-12. A block receiver carries its kind
/// *and* its position, since there is no ambient context to resolve it from
/// and it needs no `executor_proof`: a block position is self-contained in
/// the command it lowers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HirEntityNbtReceiver {
    Entity {
        kind: EntityKind,
        executor_proof: HirContextStep,
    },
    Block {
        kind: BlockEntityKind,
        position: BlockPosition,
    },
}

/// One step of a checked entity-NBT path, after the root. A `Key` step is
/// always compile-time-constant (`nbt-schema-system.md` §7 — schema keys are
/// never runtime-derived); `Index`/`Match` steps may be const or runtime.
/// `Match` (BE-1) selects a list element by a schema-known compound-field
/// match (e.g. a container slot) instead of by position — same source
/// syntax as `Index` (`[Expression]`), distinguished by the schema node kind
/// at check time, not by new grammar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirEntityPathSegment {
    Key(Box<str>),
    Index(Box<HirExpression>),
    /// `match_key` is the schema-known compound field matched against
    /// (e.g. `"slot"`), fixed by the `SchemaNode::MatchList` this step
    /// narrowed through — never source syntax, so it carries no span.
    Match {
        match_key: Box<str>,
        value: Box<HirExpression>,
    },
}

/// Closed static attributes of one checked Minecraft operation occurrence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirMinecraftOperationAttributes {
    Say {
        message: MessageLiteral,
        message_origin: OriginId,
    },
    Teleport {
        position: PositionSpec,
        component_origins: [OriginId; 3],
    },
    MoveBy {
        offset: RelativeWorldOffset,
        component_origins: [OriginId; 3],
    },
}

impl HirMinecraftOperationAttributes {
    pub(super) const fn semantic_key(&self) -> MinecraftSemanticKey {
        match self {
            Self::Say { .. } => MinecraftSemanticKey::Say,
            Self::Teleport { .. } => MinecraftSemanticKey::TeleportCurrentExecutor,
            Self::MoveBy { .. } => MinecraftSemanticKey::MoveCurrentExecutorBy,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirModuleInfo {
    pub(super) id: SourceModuleId,
    pub(super) key: ModuleKey,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirFunction {
    pub(super) id: SourceFunctionId,
    pub(super) module: SourceModuleId,
    pub(super) visibility: FunctionVisibility,
    pub(super) visibility_origin: Option<OriginId>,
    pub(super) name_origin: OriginId,
    pub(super) parameter_count: usize,
    pub(super) result: FunctionResult,
    pub(super) result_origin: Option<OriginId>,
    pub(super) bindings: Box<[HirBinding]>,
    pub(super) body: HirBlock,
    /// Non-`None` only for a compiler-synthesized event-handler reward
    /// function (PS-15): the executor (and, since vanilla runs a reward
    /// as/at the triggering player, position/rotation/dimension) this
    /// function's body is checked and verified against from entry, seeded
    /// by the compiler because there is no source-level query to establish
    /// it — vanilla itself guarantees the reward runs as/at the triggering
    /// player. `capture.proof`'s `run`/`modifier_index` are a fixed
    /// sentinel, not a reference to any real run scope; checking and
    /// verification only ever compare it for equality against itself.
    pub(super) entry_capture: Option<HirExecutorCapture>,
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
    External(SourceExternalOpId),
    If(HirIf),
    Switch(HirSwitchStatement),
    While(HirWhile),
    Break,
    Continue,
    Run(HirRun),
    Destructure {
        operand: HirExpression,
        targets: Box<[HirDestructureTarget]>,
    },
    Return(Option<HirExpression>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirDestructureTarget {
    pub(super) role: HirDestructureTargetRole,
    pub(super) local: Option<LocalId>,
    pub(super) component: u32,
    pub(super) ty: ValueType,
    pub(super) name_origin: OriginId,
    pub(super) origin: OriginId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HirDestructureTargetRole {
    Const,
    Var,
    Assign,
    Discard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirSwitchStatement {
    pub(super) scrutinee: HirExpression,
    pub(super) arms: Box<[HirSwitchStatementArm]>,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirSwitchStatementArm {
    pub(super) label: HirSwitchLabel,
    pub(super) body: HirBlock,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirSwitchLabel {
    Patterns(Box<[HirSwitchPattern]>),
    Else,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirSwitchPattern {
    pub(super) kind: HirSwitchPatternKind,
    pub(super) origin: OriginId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HirSwitchPatternKind {
    IntRange {
        min: i32,
        max: i32,
    },
    EnumVariant {
        enum_: SourceEnumId,
        variant: SourceVariantId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirWhile {
    pub(super) condition: HirExpression,
    pub(super) body: HirBlock,
    pub(super) origin: OriginId,
}

/// One ordered compiler-known execution-context modifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirRunModifier {
    As {
        query: HirEntityQuery,
        origin: OriginId,
    },
    At {
        query: HirEntityQuery,
        origin: OriginId,
    },
    AtExecutor {
        kind: EntityKind,
        proof: HirContextStep,
        origin: OriginId,
    },
    Positioned {
        position: PositionSpec,
        origin: OriginId,
    },
    Rotated {
        rotation: RotationSpec,
        origin: OriginId,
    },
    In {
        dimension: DimensionKey,
        origin: OriginId,
    },
    Anchored {
        anchor: EntityAnchor,
        origin: OriginId,
    },
    Align {
        axes: Axes,
        origin: OriginId,
    },
}

/// Exact identity of one source run-modifier transition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct HirContextStep {
    pub(super) run: SourceRunId,
    pub(super) modifier_index: usize,
    pub(super) origin: OriginId,
}

pub(super) type HirExecutionContext = ExecutionContext<HirContextStep>;

/// One source occurrence of a pure semantic entity query.
///
/// The semantic query remains canonical and origin-free. `steps` retains every
/// source refinement so verification can replay construction and target failures
/// can point at the exact argument that introduced an unsupported value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirEntityQuery {
    pub(super) semantic: StaticEntityQuery,
    pub(super) steps: Vec<HirEntityQueryStep>,
}

/// One source-ordered query-construction step with exact provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirEntityQueryStep {
    Entities {
        kind: EntityKind,
        origin: OriginId,
        kind_origin: OriginId,
    },
    WithTag {
        tag: EntityTag,
        origin: OriginId,
        value_origin: OriginId,
    },
    Limit {
        maximum: NonZeroU32,
        origin: OriginId,
        value_origin: OriginId,
    },
}

/// One optional, non-escaping lexical proof of the current executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirExecutorCapture {
    pub(super) ty: ExecutorType,
    pub(super) proof: HirContextStep,
    pub(super) name_origin: OriginId,
}

/// One structured contextual region whose modifiers retain exact source order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirRun {
    pub(super) id: SourceRunId,
    pub(super) modifiers: Box<[HirRunModifier]>,
    pub(super) resulting_context: HirExecutionContext,
    pub(super) capture: Option<HirExecutorCapture>,
    pub(super) body: HirBlock,
    pub(super) origin: OriginId,
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
pub(super) struct HirSwitchExpression {
    pub(super) scrutinee: Box<HirExpression>,
    pub(super) arms: Box<[HirSwitchExpressionArm]>,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirSwitchExpressionArm {
    pub(super) label: HirSwitchLabel,
    pub(super) body: HirExpression,
    pub(super) origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirExpressionKind {
    Bool(bool),
    Int32(i32),
    EnumVariant {
        enum_: SourceEnumId,
        variant: SourceVariantId,
    },
    Local(LocalId),
    Call(HirCall),
    External(SourceExternalOpId),
    StructConstruct {
        struct_: SourceStructId,
        fields: Box<[HirStructFieldValue]>,
    },
    StructProject {
        aggregate: Box<HirExpression>,
        field: u32,
    },
    AnonymousStructConstruct {
        struct_: SourceAnonymousStructId,
        fields: Box<[HirStructFieldValue]>,
    },
    AnonymousStructProject {
        aggregate: Box<HirExpression>,
        field: u32,
    },
    ListI32 {
        op: HirListI32Op,
        operands: Box<[HirExpression]>,
    },
    String {
        op: HirStringOp,
        operands: Box<[HirExpression]>,
    },
    Not(Box<HirExpression>),
    WrappingArithmetic {
        op: HirWrappingArithmeticOp,
        left: Box<HirExpression>,
        right: Box<HirExpression>,
    },
    Compare {
        op: HirComparisonOp,
        left: Box<HirExpression>,
        right: Box<HirExpression>,
    },
    Switch(HirSwitchExpression),
    Index {
        aggregate: Box<HirExpression>,
        component: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HirListI32Op {
    Empty,
    Length,
    Push,
    LastOrZero,
    WithoutLast,
}

impl HirListI32Op {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Length => "length",
            Self::Push => "push",
            Self::LastOrZero => "last_or_zero",
            Self::WithoutLast => "without_last",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HirStringOp {
    Constant(Box<str>),
    Length,
    EndsWithAscii(u8),
    WithoutLastUnit,
}

impl HirStringOp {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Constant(_) => "constant",
            Self::Length => "length",
            Self::EndsWithAscii(_) => "ends_with_ascii",
            Self::WithoutLastUnit => "without_last_unit",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HirStructFieldValue {
    pub(super) field: u32,
    pub(super) value: HirExpression,
    pub(super) origin: OriginId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HirWrappingArithmeticOp {
    Add,
    Subtract,
}

impl HirWrappingArithmeticOp {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+%",
            Self::Subtract => "-%",
        }
    }
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

    #[allow(
        clippy::too_many_lines,
        reason = "the deterministic HIR dump lists each complete package inventory in source order"
    )]
    fn dump(mut self) -> String {
        self.line(
            0,
            &format!(
                "package root=@{} {}",
                self.output.module.root.index(),
                self.location(self.output.module.origin)
            ),
        );
        for module in &self.output.module.modules {
            self.line(
                1,
                &format!(
                    "module @{} key={:?} {}",
                    module.id.index(),
                    module.key.as_str(),
                    self.location(module.origin)
                ),
            );
        }
        for struct_ in self.output.structs() {
            self.line(
                1,
                &format!(
                    "struct @{} {} module=@{} {}",
                    struct_.id.index(),
                    self.spelling(struct_.name_origin),
                    struct_.module.index(),
                    self.location(struct_.origin)
                ),
            );
            for (index, field) in struct_.fields.iter().enumerate() {
                self.line(
                    2,
                    &format!(
                        "field {index} {}: {} type={} {}",
                        self.spelling(field.name_origin),
                        field.ty,
                        self.location(field.type_origin),
                        self.location(field.origin)
                    ),
                );
            }
        }
        for enum_ in self.output.enums() {
            self.line(
                1,
                &format!(
                    "enum @{} {} module=@{} {}",
                    enum_.id.index(),
                    self.spelling(enum_.name_origin),
                    enum_.module.index(),
                    self.location(enum_.origin)
                ),
            );
            for variant in &enum_.variants {
                self.line(
                    2,
                    &format!(
                        "variant @{} {} {}",
                        variant.id.index(),
                        self.spelling(variant.name_origin),
                        self.location(variant.origin)
                    ),
                );
            }
        }
        for external in self.output.external_ops() {
            match &external.semantic {
                HirExternalSemantic::UnsafeMinecraftCommand {
                    command,
                    command_origin,
                } => self.line(
                    1,
                    &format!(
                        "external @{} unsafe.minecraft {:?} command={} {}",
                        external.id.index(),
                        command,
                        self.location(*command_origin),
                        self.location(external.origin)
                    ),
                ),
                HirExternalSemantic::MinecraftOperation {
                    key,
                    receiver_kind,
                    executor_proof,
                    attributes,
                    call_origin,
                    member_origin,
                    receiver_origin,
                } => {
                    let attributes = match attributes {
                        HirMinecraftOperationAttributes::Say {
                            message,
                            message_origin,
                        } => format!(
                            "message={:?} message_origin={}",
                            message.as_str(),
                            self.location(*message_origin)
                        ),
                        HirMinecraftOperationAttributes::Teleport { position, .. } => {
                            format!("position={position:?}")
                        }
                        HirMinecraftOperationAttributes::MoveBy { offset, .. } => {
                            format!("offset={offset:?}")
                        }
                    };
                    self.line(
                        1,
                        &format!(
                            "external @{} minecraft.{key:?} receiver=Executor<{receiver_kind}> proof=run@{}:modifier{} {attributes} call={} member={} receiver_origin={} {}",
                            external.id.index(),
                            executor_proof.run.index(),
                            executor_proof.modifier_index,
                            self.location(*call_origin),
                            self.location(*member_origin),
                            self.location(*receiver_origin),
                            self.location(external.origin),
                        ),
                    );
                }
                HirExternalSemantic::EntityNbtRead {
                    receiver,
                    segments,
                    result_ty,
                    receiver_origin,
                } => {
                    let path = dump_entity_nbt_path(segments);
                    let receiver_text = dump_entity_nbt_receiver(receiver);
                    self.line(
                        1,
                        &format!(
                            "external @{} entity-nbt-read receiver={receiver_text} path={path} result_ty={result_ty} receiver_origin={} {}",
                            external.id.index(),
                            self.location(*receiver_origin),
                            self.location(external.origin),
                        ),
                    );
                }
                HirExternalSemantic::EntityNbtWrite {
                    receiver,
                    segments,
                    item_id,
                    count,
                    receiver_origin,
                } => {
                    let path = dump_entity_nbt_path(segments);
                    let receiver_text = dump_entity_nbt_receiver(receiver);
                    let item_text = match &item_id.kind {
                        HirExpressionKind::String {
                            op: HirStringOp::Constant(text),
                            ..
                        } => format!("{text:?}"),
                        _ => "<runtime>".to_string(),
                    };
                    let count_text = match count.kind {
                        HirExpressionKind::Int32(value) => value.to_string(),
                        _ => "<runtime>".to_string(),
                    };
                    self.line(
                        1,
                        &format!(
                            "external @{} entity-nbt-write receiver={receiver_text} path={path} item={item_text} count={count_text} receiver_origin={} {}",
                            external.id.index(),
                            self.location(*receiver_origin),
                            self.location(external.origin),
                        ),
                    );
                }
            }
        }
        for function in self.output.functions() {
            self.dump_function(function);
        }
        for handler in self.output.event_handlers() {
            self.dump_event_handler(handler);
        }
        self.text
    }

    fn dump_event_handler(&mut self, handler: &HirEventHandler) {
        let criterion = match &handler.criterion {
            HirCriterion::InventoryChanged {
                items,
                items_origin,
            } => format!(
                "inventory_changed items={items:?} items_origin={}",
                self.location(*items_origin)
            ),
        };
        self.line(
            1,
            &format!(
                "on reward=@{} {criterion} {}",
                handler.reward.index(),
                self.location(handler.origin)
            ),
        );
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
                "fn @{} {}({}) -> {} module=@{} visibility={} {}",
                function.id.index(),
                self.spelling(function.name_origin),
                parameters.join(", "),
                function.result,
                function.module.index(),
                function.visibility,
                self.location(function.origin)
            ),
        );
        if let Some(capture) = &function.entry_capture {
            self.line(
                2,
                &format!(
                    "entry-capture Executor<{}> proof=run@{}:modifier{} {}",
                    capture.ty.kind(),
                    capture.proof.run.index(),
                    capture.proof.modifier_index,
                    self.location(capture.name_origin)
                ),
            );
        }
        let behavior = self
            .output
            .function_behavior(function.id)
            .map_or_else(|| "<missing>".to_owned(), render_behavior);
        self.line(2, &format!("behavior {behavior}"));
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

    #[allow(
        clippy::too_many_lines,
        reason = "the test-facing dump exhaustively renders the closed HIR statement vocabulary"
    )]
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
            HirStatementKind::External(external) => self.line(
                indent,
                &format!(
                    "external @{} {}",
                    external.index(),
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
            HirStatementKind::Switch(switch) => {
                self.line(
                    indent,
                    &format!(
                        "switch {} {}",
                        self.expression(&switch.scrutinee),
                        self.location(switch.origin)
                    ),
                );
                for arm in &switch.arms {
                    self.line(
                        indent + 1,
                        &format!("case {:?} {}", arm.label, self.location(arm.origin)),
                    );
                    self.dump_block(&arm.body, indent + 2);
                }
            }
            HirStatementKind::While(statement) => {
                self.line(
                    indent,
                    &format!(
                        "while {} {}",
                        self.expression(&statement.condition),
                        self.location(statement.origin)
                    ),
                );
                self.dump_block(&statement.body, indent + 1);
            }
            HirStatementKind::Break => self.line(
                indent,
                &format!("break {}", self.location(statement.origin)),
            ),
            HirStatementKind::Continue => self.line(
                indent,
                &format!("continue {}", self.location(statement.origin)),
            ),
            HirStatementKind::Destructure { operand, targets } => {
                let mut parts: Vec<String> = Vec::new();
                for target in targets {
                    let prefix = match target.role {
                        HirDestructureTargetRole::Const => "const ",
                        HirDestructureTargetRole::Var => "var ",
                        HirDestructureTargetRole::Assign => "",
                        HirDestructureTargetRole::Discard => "_",
                    };
                    if target.role == HirDestructureTargetRole::Discard {
                        parts.push(prefix.to_owned());
                    } else if let Some(local) = target.local {
                        parts.push(format!("{prefix}%{}", local.index()));
                    }
                }
                self.line(
                    indent,
                    &format!(
                        "destructure ({}) <- {} {}",
                        parts.join(", "),
                        self.expression(operand),
                        self.location(statement.origin)
                    ),
                );
            }
            HirStatementKind::Run(run) => self.dump_run(run, indent),
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

    fn dump_run(&mut self, run: &HirRun, indent: usize) {
        self.line(
            indent,
            &format!("run @{} {}", run.id.index(), self.location(run.origin)),
        );
        for modifier in &run.modifiers {
            match modifier {
                HirRunModifier::As { query, origin } => {
                    self.line(
                        indent + 1,
                        &format!("as {} {}", query.semantic, self.location(*origin)),
                    );
                    for step in &query.steps {
                        let rendered = match step {
                            HirEntityQueryStep::Entities {
                                kind,
                                origin,
                                kind_origin,
                            } => format!(
                                "query.entities {kind} {} kind={}",
                                self.location(*origin),
                                self.location(*kind_origin)
                            ),
                            HirEntityQueryStep::WithTag {
                                tag,
                                origin,
                                value_origin,
                            } => format!(
                                "query.with_tag {:?} {} value={}",
                                tag.as_str(),
                                self.location(*origin),
                                self.location(*value_origin)
                            ),
                            HirEntityQueryStep::Limit {
                                maximum,
                                origin,
                                value_origin,
                            } => format!(
                                "query.limit {maximum} {} value={}",
                                self.location(*origin),
                                self.location(*value_origin)
                            ),
                        };
                        self.line(indent + 2, &rendered);
                    }
                }
                HirRunModifier::At { query, origin } => self.line(
                    indent + 1,
                    &format!("at {} {}", query.semantic, self.location(*origin)),
                ),
                HirRunModifier::AtExecutor { kind, origin, .. } => self.line(
                    indent + 1,
                    &format!("at_executor {kind} {}", self.location(*origin)),
                ),
                HirRunModifier::Positioned { position, origin } => self.line(
                    indent + 1,
                    &format!("positioned {position:?} {}", self.location(*origin)),
                ),
                HirRunModifier::Rotated { rotation, origin } => self.line(
                    indent + 1,
                    &format!("rotated {rotation:?} {}", self.location(*origin)),
                ),
                HirRunModifier::In { dimension, origin } => self.line(
                    indent + 1,
                    &format!("in {dimension:?} {}", self.location(*origin)),
                ),
                HirRunModifier::Anchored { anchor, origin } => self.line(
                    indent + 1,
                    &format!("anchored {anchor:?} {}", self.location(*origin)),
                ),
                HirRunModifier::Align { axes, origin } => self.line(
                    indent + 1,
                    &format!("align {} {}", axes.as_str(), self.location(*origin)),
                ),
            }
        }
        self.line(indent + 1, &format!("context {:?}", run.resulting_context));
        if let Some(capture) = &run.capture {
            self.line(
                indent + 1,
                &format!(
                    "capture {}: Executor<{}> proof={:?} {}",
                    self.spelling(capture.name_origin),
                    capture.ty.kind(),
                    capture.proof,
                    self.location(capture.name_origin)
                ),
            );
        }
        self.dump_block(&run.body, indent + 1);
    }

    fn expression(&self, expression: &HirExpression) -> String {
        let value = match &expression.kind {
            HirExpressionKind::Bool(value) => value.to_string(),
            HirExpressionKind::Int32(value) => value.to_string(),
            HirExpressionKind::EnumVariant { enum_, variant } => {
                format!("enum@{}.{}", enum_.index(), variant.index())
            }
            HirExpressionKind::Local(local) => format!("%{}", local.index()),
            HirExpressionKind::Call(call) => self.call(call),
            HirExpressionKind::External(operation) => format!("external@{}", operation.index()),
            HirExpressionKind::StructConstruct { struct_, fields } => {
                let fields = fields
                    .iter()
                    .map(|field| format!("{}={}", field.field, self.expression(&field.value)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("struct@{}{{{fields}}}", struct_.index())
            }
            HirExpressionKind::StructProject { aggregate, field } => {
                format!("({}).field{field}", self.expression(aggregate))
            }
            HirExpressionKind::AnonymousStructConstruct { struct_, fields } => {
                let fields = fields
                    .iter()
                    .map(|field| format!("{}={}", field.field, self.expression(&field.value)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("anon.struct@{}{{{fields}}}", struct_.index())
            }
            HirExpressionKind::AnonymousStructProject { aggregate, field } => {
                format!("({})[{field}]", self.expression(aggregate))
            }
            HirExpressionKind::ListI32 { op, operands } => {
                let operands = operands
                    .iter()
                    .map(|operand| self.expression(operand))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("list.i32.{}({operands})", op.as_str())
            }
            HirExpressionKind::String { op, operands } => {
                let operands = operands
                    .iter()
                    .map(|operand| self.expression(operand))
                    .collect::<Vec<_>>()
                    .join(", ");
                match op {
                    HirStringOp::Constant(value) => format!("string.constant({value:?})"),
                    HirStringOp::EndsWithAscii(ascii) => {
                        format!("string.ends_with_ascii({operands}, {ascii})")
                    }
                    _ => format!("string.{}({operands})", op.as_str()),
                }
            }
            HirExpressionKind::Not(operand) => format!("!({})", self.expression(operand)),
            HirExpressionKind::WrappingArithmetic { op, left, right } => format!(
                "({} {} {})",
                self.expression(left),
                op.as_str(),
                self.expression(right)
            ),
            HirExpressionKind::Compare { op, left, right } => format!(
                "({} {} {})",
                self.expression(left),
                op.as_str(),
                self.expression(right)
            ),
            HirExpressionKind::Switch(switch) => format!(
                "switch({}; {} arms)",
                self.expression(&switch.scrutinee),
                switch.arms.len()
            ),
            HirExpressionKind::Index {
                aggregate,
                component,
            } => {
                format!("({})[{}]", self.expression(aggregate), component)
            }
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

fn render_behavior(behavior: FunctionBehavior) -> String {
    format!(
        "context={:?} world={:?} observable={:?} fork={:?} work={:?} unsafe_unknown={}",
        behavior.required_ambient_context(),
        behavior.world_effect(),
        behavior.observable_effect(),
        behavior.fork_bound(),
        behavior.transitive_work(),
        behavior.contains_unsafe_unknown()
    )
}

/// Renders an entity-NBT path's segments, dot/bracket style — shared by both
/// `EntityNbtRead` and `EntityNbtWrite`'s dump text (PS-16).
fn dump_entity_nbt_path(segments: &[HirEntityPathSegment]) -> String {
    segments
        .iter()
        .map(|segment| match segment {
            HirEntityPathSegment::Key(key) => format!(".{key}"),
            HirEntityPathSegment::Index(index) => match &index.kind {
                HirExpressionKind::Int32(value) => format!("[{value}]"),
                _ => "[<runtime>]".to_string(),
            },
            HirEntityPathSegment::Match { match_key, value } => match &value.kind {
                HirExpressionKind::Int32(n) => format!("[{{{match_key}:{n}}}]"),
                _ => format!("[{{{match_key}:<runtime>}}]"),
            },
        })
        .collect::<String>()
}

/// Renders an entity-NBT path's root receiver — shared by both
/// `EntityNbtRead` and `EntityNbtWrite`'s dump text (PS-16).
fn dump_entity_nbt_receiver(receiver: &HirEntityNbtReceiver) -> String {
    match receiver {
        HirEntityNbtReceiver::Entity {
            kind,
            executor_proof,
        } => format!(
            "Executor<{kind}> proof=run@{}:modifier{}",
            executor_proof.run.index(),
            executor_proof.modifier_index
        ),
        HirEntityNbtReceiver::Block { kind, position } => {
            format!(
                "Block<{kind}> position={} {} {}",
                position.x, position.y, position.z
            )
        }
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
    #[allow(
        clippy::too_many_lines,
        reason = "the package verifier checks dense global inventories and recomputed summaries in one ordered boundary"
    )]
    fn verify(&self) -> Result<(), HirVerificationError> {
        self.origin(self.output.module.origin, "package")?;
        let root_index = self
            .output
            .module
            .root
            .as_usize()
            .filter(|index| self.output.module.modules.get(*index).is_some())
            .ok_or_else(|| HirVerificationError::new("package has an invalid root module"))?;
        for (index, module) in self.output.module.modules.iter().enumerate() {
            let expected = SourceModuleId::from_index(index)
                .ok_or_else(|| HirVerificationError::new("module identity space exhausted"))?;
            if module.id != expected {
                return Err(HirVerificationError::new(format!(
                    "module at index {index} has non-dense identity {:?}",
                    module.id
                )));
            }
            self.origin(module.origin, "module")?;
        }
        if self.output.module.modules[root_index].id != self.output.module.root {
            return Err(HirVerificationError::new(
                "package root does not match its module inventory entry",
            ));
        }
        for (index, struct_) in self.output.structs().iter().enumerate() {
            let expected = SourceStructId::from_index(index)
                .ok_or_else(|| HirVerificationError::new("struct identity space exhausted"))?;
            if struct_.id != expected {
                return Err(HirVerificationError::new(format!(
                    "struct at index {index} has non-dense identity {:?}",
                    struct_.id
                )));
            }
            if struct_
                .module
                .as_usize()
                .is_none_or(|module| module >= self.output.module.modules.len())
            {
                return Err(HirVerificationError::new(format!(
                    "struct {:?} has invalid owner module {:?}",
                    struct_.id, struct_.module
                )));
            }
            self.origin(struct_.origin, "struct")?;
            self.origin(struct_.name_origin, "struct name")?;
            if struct_.fields.len() > 64 {
                return Err(HirVerificationError::new(format!(
                    "struct {:?} exceeds the Phase-1 field limit",
                    struct_.id
                )));
            }
            for field in &struct_.fields {
                self.origin(field.origin, "struct field")?;
                self.origin(field.name_origin, "struct field name")?;
                self.origin(field.type_origin, "struct field type")?;
                self.verify_value_type(field.ty, &mut vec![struct_.id], 1)?;
            }
        }
        for (index, enum_) in self.output.enums().iter().enumerate() {
            let expected = SourceEnumId::from_index(index)
                .ok_or_else(|| HirVerificationError::new("enum identity space exhausted"))?;
            if enum_.id != expected || enum_.variants.is_empty() {
                return Err(HirVerificationError::new(
                    "enum inventory is non-dense or empty",
                ));
            }
            if enum_
                .module
                .as_usize()
                .is_none_or(|module| module >= self.output.module.modules.len())
            {
                return Err(HirVerificationError::new("enum has invalid owner module"));
            }
            self.origin(enum_.origin, "enum")?;
            self.origin(enum_.name_origin, "enum name")?;
            for (variant_index, variant) in enum_.variants.iter().enumerate() {
                if variant.id
                    != SourceVariantId::from_index(variant_index).ok_or_else(|| {
                        HirVerificationError::new("variant identity space exhausted")
                    })?
                {
                    return Err(HirVerificationError::new(
                        "enum variant inventory is non-dense",
                    ));
                }
                self.origin(variant.origin, "enum variant")?;
                self.origin(variant.name_origin, "enum variant name")?;
            }
        }
        for (index, external) in self.output.external_ops().iter().enumerate() {
            let expected = SourceExternalOpId::from_index(index).ok_or_else(|| {
                HirVerificationError::new("external-operation identity space exhausted")
            })?;
            if external.id != expected {
                return Err(HirVerificationError::new(format!(
                    "external operation at index {index} has non-dense identity {:?}",
                    external.id
                )));
            }
            self.origin(external.origin, "external operation")?;
            match &external.semantic {
                HirExternalSemantic::UnsafeMinecraftCommand {
                    command,
                    command_origin,
                } => {
                    self.origin(*command_origin, "unsafe command literal")?;
                    validate_command_line_shape(command).map_err(|error| {
                        HirVerificationError::new(format!(
                            "unsafe command {:?} has invalid physical shape: {error:?}",
                            external.id
                        ))
                    })?;
                }
                HirExternalSemantic::MinecraftOperation {
                    key,
                    receiver_kind,
                    executor_proof,
                    attributes,
                    call_origin,
                    member_origin,
                    receiver_origin,
                } => {
                    self.origin(*call_origin, "Minecraft method call")?;
                    self.origin(*member_origin, "Minecraft method member")?;
                    self.origin(*receiver_origin, "Minecraft method receiver")?;
                    self.origin(executor_proof.origin, "Minecraft executor proof")?;
                    if attributes.semantic_key() != *key
                        || minecraft_descriptor(*key).key() != *key
                        || !receiver_kind
                            .capabilities()
                            .contains(EntityCapability::CommandExecutor)
                    {
                        return Err(HirVerificationError::new(format!(
                            "Minecraft operation {:?} has mismatched key, attributes, or receiver",
                            external.id
                        )));
                    }
                    match attributes {
                        HirMinecraftOperationAttributes::Say {
                            message,
                            message_origin,
                        } => {
                            self.origin(*message_origin, "Minecraft message literal")?;
                            crate::ir::semantic::MessageLiteral::new(message.as_str()).map_err(
                                |error| {
                                    HirVerificationError::new(format!(
                                        "Minecraft message in {:?} is invalid: {error}",
                                        external.id
                                    ))
                                },
                            )?;
                        }
                        HirMinecraftOperationAttributes::Teleport {
                            component_origins, ..
                        }
                        | HirMinecraftOperationAttributes::MoveBy {
                            component_origins, ..
                        } => {
                            for origin in component_origins {
                                self.origin(*origin, "Minecraft spatial component")?;
                            }
                        }
                    }
                }
                HirExternalSemantic::EntityNbtRead {
                    receiver,
                    segments,
                    result_ty: _,
                    receiver_origin,
                } => {
                    self.origin(*receiver_origin, "entity-NBT path receiver")?;
                    match receiver {
                        HirEntityNbtReceiver::Entity {
                            kind,
                            executor_proof,
                        } => {
                            self.origin(executor_proof.origin, "entity-NBT path executor proof")?;
                            if !kind
                                .capabilities()
                                .contains(EntityCapability::CommandExecutor)
                            {
                                return Err(HirVerificationError::new(format!(
                                    "entity-NBT path {:?} has a receiver kind without executor capability",
                                    external.id
                                )));
                            }
                        }
                        HirEntityNbtReceiver::Block { .. } => {}
                    }
                    if segments.is_empty() {
                        return Err(HirVerificationError::new(format!(
                            "entity-NBT path {:?} has no path segments",
                            external.id
                        )));
                    }
                    verify_entity_nbt_segments_are_int32(segments, external.id)?;
                }
                HirExternalSemantic::EntityNbtWrite {
                    receiver,
                    segments,
                    item_id,
                    count,
                    receiver_origin,
                } => {
                    self.origin(*receiver_origin, "entity-NBT write receiver")?;
                    if !matches!(receiver, HirEntityNbtReceiver::Block { .. }) {
                        return Err(HirVerificationError::new(format!(
                            "entity-NBT write {:?} has a non-block receiver",
                            external.id
                        )));
                    }
                    if segments.is_empty() {
                        return Err(HirVerificationError::new(format!(
                            "entity-NBT write {:?} has no path segments",
                            external.id
                        )));
                    }
                    if item_id.ty != ValueType::String {
                        return Err(HirVerificationError::new(format!(
                            "entity-NBT write {:?} has a non-String item id",
                            external.id
                        )));
                    }
                    if let HirExpressionKind::String {
                        op: HirStringOp::Constant(text),
                        ..
                    } = &item_id.kind
                    {
                        if text.is_empty() {
                            return Err(HirVerificationError::new(format!(
                                "entity-NBT write {:?} has an empty item id",
                                external.id
                            )));
                        }
                    }
                    if count.ty != ValueType::Int32 {
                        return Err(HirVerificationError::new(format!(
                            "entity-NBT write {:?} has a non-Int32 count",
                            external.id
                        )));
                    }
                    verify_entity_nbt_segments_are_int32(segments, external.id)?;
                }
            }
        }
        for (index, function) in self.output.functions().iter().enumerate() {
            let expected = SourceFunctionId::from_index(index)
                .ok_or_else(|| HirVerificationError::new("function identity space exhausted"))?;
            if function.id != expected {
                return Err(HirVerificationError::new(format!(
                    "function at index {index} has non-dense identity {:?}",
                    function.id
                )));
            }
        }
        if self.output.module.behaviors.len() != self.output.module.functions.len() {
            return Err(HirVerificationError::new(format!(
                "package has {} function behavior summaries for {} functions",
                self.output.module.behaviors.len(),
                self.output.module.functions.len()
            )));
        }
        let mut next_run_scope = 0_usize;
        for function in self.output.functions() {
            verify_run_scope_ids(&function.body, &mut next_run_scope)?;
        }
        if next_run_scope != self.output.module.run_scope_count {
            return Err(HirVerificationError::new(format!(
                "package inventories {} run scopes but contains {next_run_scope}",
                self.output.module.run_scope_count
            )));
        }
        for function in self.output.functions() {
            self.verify_function(function)?;
        }
        verify_external_operation_occurrences(self.output.functions(), self.output.external_ops())?;
        let inferred = super::behavior::infer_function_behaviors(
            &self.output.module.functions,
            &self.output.module.external_ops,
        )
        .map_err(|error| {
            HirVerificationError::new(format!(
                "cannot recompute function behavior summaries: {error}"
            ))
        })?;
        if inferred != self.output.module.behaviors {
            let mismatch = inferred
                .iter()
                .zip(self.output.module.behaviors.iter())
                .position(|(expected, actual)| expected != actual)
                .unwrap_or(0);
            return Err(HirVerificationError::new(format!(
                "function behavior summary at index {mismatch} does not match HIR inference"
            )));
        }
        Ok(())
    }

    fn verify_value_type(
        &self,
        ty: ValueType,
        active: &mut Vec<SourceStructId>,
        depth: usize,
    ) -> Result<(), HirVerificationError> {
        if let ValueType::Enum(enum_id) = ty {
            return self.output.enum_(enum_id).map(|_| ()).ok_or_else(|| {
                HirVerificationError::new(format!("value type names missing enum {enum_id:?}"))
            });
        }
        let ValueType::Struct(struct_id) = ty else {
            return Ok(());
        };
        if depth > 16 {
            return Err(HirVerificationError::new(format!(
                "aggregate type exceeds the Phase-1 nesting-depth limit at {struct_id:?}"
            )));
        }
        if active.contains(&struct_id) {
            return Err(HirVerificationError::new(format!(
                "aggregate type contains a recursive cycle through {struct_id:?}"
            )));
        }
        let declaration = self.output.struct_(struct_id).ok_or_else(|| {
            HirVerificationError::new(format!("aggregate type names missing {struct_id:?}"))
        })?;
        active.push(struct_id);
        for field in &declaration.fields {
            self.verify_value_type(field.ty, active, depth + 1)?;
        }
        active.pop();
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "function inventory and body verification remain one ordered invariant boundary"
    )]
    fn verify_function(&self, function: &HirFunction) -> Result<(), HirVerificationError> {
        self.origin(function.origin, "function")?;
        self.origin(function.name_origin, "function name")?;
        let module_index = function
            .module
            .as_usize()
            .filter(|index| self.output.module.modules.get(*index).is_some())
            .ok_or_else(|| {
                HirVerificationError::new(format!(
                    "function {:?} has invalid owner module {:?}",
                    function.id, function.module
                ))
            })?;
        if self.output.module.modules[module_index].id != function.module {
            return Err(HirVerificationError::new(format!(
                "function {:?} owner module does not match the module inventory",
                function.id
            )));
        }
        match (function.visibility, function.visibility_origin) {
            (FunctionVisibility::Private, None) => {}
            (FunctionVisibility::Public | FunctionVisibility::DatapackExport, Some(origin)) => {
                self.origin(origin, "function visibility")?;
            }
            (FunctionVisibility::Private, Some(_)) => {
                return Err(HirVerificationError::new(format!(
                    "private function {:?} has a visibility origin",
                    function.id
                )));
            }
            (FunctionVisibility::Public | FunctionVisibility::DatapackExport, None) => {
                return Err(HirVerificationError::new(format!(
                    "visible function {:?} has no visibility origin",
                    function.id
                )));
            }
        }
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
            self.verify_value_type(binding.ty, &mut vec![], 0)?;
        }
        if function.visibility == FunctionVisibility::DatapackExport
            && (function.bindings[..function.parameter_count]
                .iter()
                .any(|binding| self.value_type_contains_enum(binding.ty, &mut vec![]))
                || matches!(function.result, FunctionResult::Value(ty) if self.value_type_contains_enum(ty, &mut vec![])))
        {
            return Err(HirVerificationError::new(
                "datapack export exposes an enum-containing ABI type",
            ));
        }

        let mut state = VerifyState::new(function.bindings.len());
        for index in 0..function.parameter_count {
            state.set_active(index, true);
            state.set_assigned(index, true);
        }
        let mut next_declaration = function.parameter_count;
        let context = match &function.entry_capture {
            Some(capture) => {
                self.origin(capture.proof.origin, "event-handler entry proof")?;
                self.origin(capture.name_origin, "event-handler entry capture")?;
                function_entry_context()
                    .establish_executor(capture.ty.kind(), capture.proof)
                    .establish_position(capture.proof)
                    .establish_rotation(capture.proof)
                    .establish_dimension(capture.proof)
            }
            None => function_entry_context(),
        };
        let continues = self.verify_block(
            function,
            &function.body,
            &mut state,
            &mut next_declaration,
            &context,
            0,
        )?;
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

    fn value_type_contains_enum(&self, ty: ValueType, active: &mut Vec<SourceStructId>) -> bool {
        match ty {
            ValueType::Enum(_) => true,
            ValueType::Struct(id) => {
                if active.contains(&id) {
                    return false;
                }
                active.push(id);
                let result = self.output.struct_(id).is_some_and(|struct_| {
                    struct_
                        .fields
                        .iter()
                        .any(|field| self.value_type_contains_enum(field.ty, active))
                });
                active.pop();
                result
            }
            _ => false,
        }
    }

    fn verify_block(
        &self,
        function: &HirFunction,
        block: &HirBlock,
        state: &mut VerifyState,
        next_declaration: &mut usize,
        context: &HirExecutionContext,
        loop_depth: usize,
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
                context,
                loop_depth,
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

    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "the exhaustive statement verifier explicitly threads lexical, flow, context, and loop state"
    )]
    fn verify_statement(
        &self,
        function: &HirFunction,
        statement: &HirStatement,
        state: &mut VerifyState,
        next_declaration: &mut usize,
        scope_locals: &mut Vec<LocalId>,
        context: &HirExecutionContext,
        loop_depth: usize,
    ) -> Result<bool, HirVerificationError> {
        self.origin(statement.origin, "statement")?;
        match &statement.kind {
            HirStatementKind::Declaration { local, initializer } => {
                self.verify_declaration(
                    function,
                    *local,
                    initializer.as_ref(),
                    state,
                    next_declaration,
                    scope_locals,
                )?;
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
            HirStatementKind::External(external) => {
                let declaration = self.output.external_op(*external).ok_or_else(|| {
                    HirVerificationError::new(format!(
                        "statement references invalid external operation {external:?}"
                    ))
                })?;
                if declaration.origin != statement.origin {
                    return Err(HirVerificationError::new(format!(
                        "external statement {external:?} does not match declaration provenance"
                    )));
                }
                if let HirExternalSemantic::MinecraftOperation {
                    receiver_kind,
                    executor_proof,
                    ..
                } = &declaration.semantic
                {
                    match context.executor() {
                        crate::ir::semantic::ContextFact::Established { value, by }
                            if value == receiver_kind && by == executor_proof => {}
                        _ => {
                            return Err(HirVerificationError::new(format!(
                                "Minecraft operation {external:?} does not use the exact current executor proof"
                            )));
                        }
                    }
                }
                Ok(true)
            }
            HirStatementKind::If(conditional) => self.verify_if(
                function,
                conditional,
                state,
                next_declaration,
                context,
                loop_depth,
            ),
            HirStatementKind::Switch(switch) => self.verify_switch_statement(
                function,
                switch,
                state,
                next_declaration,
                context,
                loop_depth,
            ),
            HirStatementKind::While(loop_) => {
                if loop_.origin != statement.origin {
                    return Err(HirVerificationError::new(
                        "while statement and region provenance differ",
                    ));
                }
                self.origin(loop_.origin, "while loop")?;
                self.expression(function, &loop_.condition, state)?;
                if loop_.condition.ty != ValueType::Bool {
                    return Err(HirVerificationError::new("while condition is not Bool"));
                }
                let checkpoint = state.checkpoint();
                let _ = self.verify_block(
                    function,
                    &loop_.body,
                    state,
                    next_declaration,
                    context,
                    loop_depth.saturating_add(1),
                )?;
                state.rollback(checkpoint);
                Ok(true)
            }
            HirStatementKind::Break | HirStatementKind::Continue => {
                if loop_depth == 0 {
                    return Err(HirVerificationError::new(
                        "loop control appears outside a loop",
                    ));
                }
                Ok(false)
            }
            HirStatementKind::Destructure { operand, targets } => {
                self.expression(function, operand, state)?;
                let ValueType::AnonymousStruct(struct_) = operand.ty else {
                    return Err(HirVerificationError::new(
                        "destructure operand is not a positional anonymous struct",
                    ));
                };
                let declaration = self.output.anonymous_struct(struct_).ok_or_else(|| {
                    HirVerificationError::new("destructure operand has invalid anonymous struct")
                })?;
                let HirAnonymousStructKind::Positional(components) = &declaration.kind else {
                    return Err(HirVerificationError::new(
                        "destructure operand is a named, not positional, anonymous struct",
                    ));
                };
                if targets.len() != components.len() {
                    return Err(HirVerificationError::new(format!(
                        "destructure has {} targets but operand has {} components",
                        targets.len(),
                        components.len()
                    )));
                }
                for target in targets {
                    let component_index = target.component as usize;
                    if component_index >= components.len() {
                        return Err(HirVerificationError::new(format!(
                            "destructure target references component {component_index}, operand has {} components",
                            components.len()
                        )));
                    }
                    if target.ty != components[component_index] {
                        return Err(HirVerificationError::new(format!(
                            "destructure target has type {}, expected {}",
                            target.ty, components[component_index]
                        )));
                    }
                    if let Some(local) = target.local {
                        match target.role {
                            HirDestructureTargetRole::Const | HirDestructureTargetRole::Var => {
                                let index = Self::local_index(function, local)?;
                                if index != *next_declaration {
                                    return Err(HirVerificationError::new(format!(
                                        "destructure binding {local:?} is out of source order; expected binding index {}",
                                        *next_declaration
                                    )));
                                }
                                *next_declaration += 1;
                                state.set_active(index, true);
                                state.set_assigned(index, true);
                                scope_locals.push(local);
                            }
                            HirDestructureTargetRole::Assign => {
                                Self::active_local_index(function, local, state)?;
                                let index = Self::local_index(function, local)?;
                                state.set_assigned(index, true);
                            }
                            HirDestructureTargetRole::Discard => {}
                        }
                    }
                }
                Ok(true)
            }
            HirStatementKind::Run(run) => {
                self.verify_run(function, run, statement.origin, next_declaration, context)?;
                Ok(true)
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

    fn verify_declaration(
        &self,
        function: &HirFunction,
        local: LocalId,
        initializer: Option<&HirExpression>,
        state: &mut VerifyState,
        next_declaration: &mut usize,
        scope_locals: &mut Vec<LocalId>,
    ) -> Result<(), HirVerificationError> {
        let index = Self::local_index(function, local)?;
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
        scope_locals.push(local);
        Ok(())
    }

    fn verify_run(
        &self,
        function: &HirFunction,
        run: &HirRun,
        statement_origin: OriginId,
        next_declaration: &mut usize,
        input_context: &HirExecutionContext,
    ) -> Result<(), HirVerificationError> {
        if run.origin != statement_origin {
            return Err(HirVerificationError::new(
                "run statement and region provenance differ",
            ));
        }
        self.origin(run.origin, "run region")?;
        for modifier in &run.modifiers {
            match modifier {
                HirRunModifier::As { query, origin } => {
                    self.origin(*origin, "run as modifier")?;
                    self.verify_entity_query(query)?;
                    if !query
                        .semantic
                        .kind()
                        .capabilities()
                        .contains(EntityCapability::CommandExecutor)
                    {
                        return Err(HirVerificationError::new(format!(
                            "run query kind {} cannot establish an executor",
                            query.semantic.kind()
                        )));
                    }
                }
                HirRunModifier::At { query, origin } => {
                    self.origin(*origin, "run at modifier")?;
                    self.verify_entity_query(query)?;
                }
                HirRunModifier::AtExecutor { origin, proof, .. } => {
                    self.origin(*origin, "run at-executor modifier")?;
                    if proof.run != run.id {
                        return Err(HirVerificationError::new(
                            "at_executor proof belongs to another run",
                        ));
                    }
                }
                HirRunModifier::Positioned { origin, .. }
                | HirRunModifier::Rotated { origin, .. }
                | HirRunModifier::In { origin, .. }
                | HirRunModifier::Anchored { origin, .. }
                | HirRunModifier::Align { origin, .. } => {
                    self.origin(*origin, "spatial run modifier")?;
                }
            }
        }
        let replayed_context = apply_run_modifiers(*input_context, run.id, &run.modifiers)
            .map_err(|error| {
                HirVerificationError::new(format!(
                    "run modifier at {:?} cannot establish its promised context",
                    error.origin
                ))
            })?;
        if replayed_context != run.resulting_context {
            return Err(HirVerificationError::new(
                "run resulting context does not match its ordered modifier transfer",
            ));
        }
        self.verify_executor_capture(run, &replayed_context)?;
        if block_contains_return(&run.body) {
            return Err(HirVerificationError::new(
                "run region contains a function return",
            ));
        }

        // Run bodies are outlined into independent functions. Until explicit scalar
        // captures exist, verify them in an empty environment so corrupted HIR cannot
        // depend on an outer local that lowering has no way to supply.
        let mut isolated_state = VerifyState::new(function.bindings.len());
        let _ = self.verify_block(
            function,
            &run.body,
            &mut isolated_state,
            next_declaration,
            &replayed_context,
            0,
        )?;
        Ok(())
    }

    fn verify_entity_query(&self, query: &HirEntityQuery) -> Result<(), HirVerificationError> {
        let mut steps = query.steps.iter();
        let Some(HirEntityQueryStep::Entities {
            kind,
            origin,
            kind_origin,
        }) = steps.next()
        else {
            return Err(HirVerificationError::new(
                "entity query does not begin with exactly one entities root",
            ));
        };
        self.origin(*origin, "entity query root")?;
        self.origin(*kind_origin, "entity query kind")?;
        let mut replayed = StaticEntityQuery::entities(*kind);
        for step in steps {
            match step {
                HirEntityQueryStep::Entities { .. } => {
                    return Err(HirVerificationError::new(
                        "entity query contains a repeated entities root",
                    ));
                }
                HirEntityQueryStep::WithTag {
                    tag,
                    origin,
                    value_origin,
                } => {
                    self.origin(*origin, "entity query with_tag step")?;
                    self.origin(*value_origin, "entity query tag value")?;
                    replayed = replayed.with_tag(tag.clone());
                }
                HirEntityQueryStep::Limit {
                    maximum,
                    origin,
                    value_origin,
                } => {
                    self.origin(*origin, "entity query limit step")?;
                    self.origin(*value_origin, "entity query limit value")?;
                    replayed = replayed.limit(maximum.get()).map_err(|error| {
                        HirVerificationError::new(format!(
                            "entity query contains an invalid limit step: {error}"
                        ))
                    })?;
                }
            }
        }
        if replayed != query.semantic {
            return Err(HirVerificationError::new(
                "entity query steps do not reproduce its canonical semantics",
            ));
        }
        Ok(())
    }

    fn verify_executor_capture(
        &self,
        run: &HirRun,
        final_context: &HirExecutionContext,
    ) -> Result<(), HirVerificationError> {
        match (&run.capture, final_context.executor()) {
            (Some(capture), crate::ir::semantic::ContextFact::Established { value: kind, by })
                if capture.ty.kind() == *kind && capture.proof == *by =>
            {
                self.origin(capture.name_origin, "executor capture")?;
                Ok(())
            }
            (Some(_), crate::ir::semantic::ContextFact::Established { .. }) => {
                Err(HirVerificationError::new(
                    "executor capture kind or exact proof does not match the final run context",
                ))
            }
            (
                Some(_),
                crate::ir::semantic::ContextFact::Unavailable
                | crate::ir::semantic::ContextFact::Inherited,
            ) => Err(HirVerificationError::new(
                "run captures an executor no modifier establishes",
            )),
            (None, _) => Ok(()),
        }
    }

    fn verify_if(
        &self,
        function: &HirFunction,
        conditional: &HirIf,
        state: &mut VerifyState,
        next_declaration: &mut usize,
        context: &HirExecutionContext,
        loop_depth: usize,
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
            if self.verify_block(
                function,
                &arm.body,
                state,
                next_declaration,
                context,
                loop_depth,
            )? {
                let delta = state.newly_assigned_since(checkpoint);
                VerifyState::merge_delta_intersection(&mut continuing_delta, &delta);
            }
            state.rollback(checkpoint);
        }
        if let Some(else_body) = &conditional.else_body {
            if self.verify_block(
                function,
                else_body,
                state,
                next_declaration,
                context,
                loop_depth,
            )? {
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

    fn verify_switch_statement(
        &self,
        function: &HirFunction,
        switch: &HirSwitchStatement,
        state: &mut VerifyState,
        next_declaration: &mut usize,
        context: &HirExecutionContext,
        loop_depth: usize,
    ) -> Result<bool, HirVerificationError> {
        self.origin(switch.origin, "switch statement")?;
        self.expression(function, &switch.scrutinee, state)?;
        let mut coverage = HirSwitchCoverage::new(
            switch.scrutinee.ty,
            self.enum_variant_count(switch.scrutinee.ty)?,
        );
        let checkpoint = state.checkpoint();
        let mut continuing = None;
        for (index, arm) in switch.arms.iter().enumerate() {
            self.origin(arm.origin, "switch arm")?;
            self.verify_switch_label(
                &arm.label,
                switch.scrutinee.ty,
                &mut coverage,
                index + 1 == switch.arms.len(),
            )?;
            if self.verify_block(
                function,
                &arm.body,
                state,
                next_declaration,
                context,
                loop_depth,
            )? {
                let delta = state.newly_assigned_since(checkpoint);
                VerifyState::merge_delta_intersection(&mut continuing, &delta);
            }
            state.rollback(checkpoint);
        }
        if !coverage.complete() {
            return Err(HirVerificationError::new(
                "switch statement is not exhaustive",
            ));
        }
        if let Some(delta) = continuing {
            for index in delta {
                state.set_assigned(index, true);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn enum_variant_count(&self, ty: ValueType) -> Result<usize, HirVerificationError> {
        match ty {
            ValueType::Enum(id) => self
                .output
                .enum_(id)
                .map(|enum_| enum_.variants.len())
                .ok_or_else(|| HirVerificationError::new("switch names invalid enum")),
            ValueType::Int32 => Ok(0),
            _ => Err(HirVerificationError::new(
                "switch scrutinee has unsupported type",
            )),
        }
    }

    fn verify_switch_label(
        &self,
        label: &HirSwitchLabel,
        ty: ValueType,
        coverage: &mut HirSwitchCoverage,
        is_last: bool,
    ) -> Result<(), HirVerificationError> {
        match label {
            HirSwitchLabel::Else => {
                if coverage.has_else || !is_last || coverage.complete() {
                    return Err(HirVerificationError::new(
                        "invalid or unreachable switch else arm",
                    ));
                }
                coverage.has_else = true;
            }
            HirSwitchLabel::Patterns(patterns) => {
                if patterns.is_empty() || coverage.has_else {
                    return Err(HirVerificationError::new(
                        "empty patterns or patterns after else",
                    ));
                }
                for pattern in patterns {
                    self.origin(pattern.origin, "switch pattern")?;
                    match (pattern.kind, ty) {
                        (HirSwitchPatternKind::IntRange { min, max }, ValueType::Int32)
                            if min <= max =>
                        {
                            coverage.insert_interval(min, max)?;
                        }
                        (
                            HirSwitchPatternKind::EnumVariant { enum_, variant },
                            ValueType::Enum(expected),
                        ) if enum_ == expected => {
                            coverage.insert_variant(variant.as_usize().ok_or_else(|| {
                                HirVerificationError::new("variant ID does not fit usize")
                            })?)?;
                        }
                        _ => {
                            return Err(HirVerificationError::new(
                                "switch pattern type or range invariant is invalid",
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "HIR verification keeps the closed expression vocabulary in one exhaustive boundary"
    )]
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
            HirExpressionKind::EnumVariant { enum_, variant } => {
                let declaration = self
                    .output
                    .enum_(*enum_)
                    .ok_or_else(|| HirVerificationError::new("enum literal names invalid enum"))?;
                if variant
                    .as_usize()
                    .is_none_or(|index| index >= declaration.variants.len())
                {
                    return Err(HirVerificationError::new(
                        "enum literal names invalid variant",
                    ));
                }
                ValueType::Enum(*enum_)
            }
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
            HirExpressionKind::External(operation) => {
                let external = self.output.external_op(*operation).ok_or_else(|| {
                    HirVerificationError::new(format!("invalid external identity {operation:?}"))
                })?;
                match &external.semantic {
                    HirExternalSemantic::EntityNbtRead { result_ty, .. } => *result_ty,
                    _ => {
                        return Err(HirVerificationError::new(
                            "non-value external operation appears in an expression",
                        ));
                    }
                }
            }
            HirExpressionKind::StructConstruct { struct_, fields } => {
                let declaration = self.output.struct_(*struct_).ok_or_else(|| {
                    HirVerificationError::new(format!("invalid struct identity {struct_:?}"))
                })?;
                if fields.len() != declaration.fields.len() {
                    return Err(HirVerificationError::new(
                        "struct construction field count differs from its declaration",
                    ));
                }
                let mut seen = vec![false; declaration.fields.len()];
                for field in fields {
                    self.origin(field.origin, "struct field initializer")?;
                    let index = usize::try_from(field.field).map_err(|_| {
                        HirVerificationError::new("struct field index does not fit usize")
                    })?;
                    let expected = declaration.fields.get(index).ok_or_else(|| {
                        HirVerificationError::new("struct construction has invalid field index")
                    })?;
                    if std::mem::replace(&mut seen[index], true) {
                        return Err(HirVerificationError::new(
                            "struct construction repeats a field",
                        ));
                    }
                    self.expression(function, &field.value, state)?;
                    if field.value.ty != expected.ty {
                        return Err(HirVerificationError::new(
                            "struct field initializer type differs from its declaration",
                        ));
                    }
                }
                if seen.iter().any(|seen| !seen) {
                    return Err(HirVerificationError::new(
                        "struct construction omits a field",
                    ));
                }
                ValueType::Struct(*struct_)
            }
            HirExpressionKind::StructProject { aggregate, field } => {
                self.expression(function, aggregate, state)?;
                let ValueType::Struct(struct_) = aggregate.ty else {
                    return Err(HirVerificationError::new(
                        "struct projection receiver is not a struct",
                    ));
                };
                let declaration = self.output.struct_(struct_).ok_or_else(|| {
                    HirVerificationError::new(format!("invalid struct identity {struct_:?}"))
                })?;
                declaration
                    .fields
                    .get(usize::try_from(*field).unwrap_or(usize::MAX))
                    .map(|field| field.ty)
                    .ok_or_else(|| HirVerificationError::new("invalid struct projection field"))?
            }
            HirExpressionKind::AnonymousStructConstruct { struct_, fields } => {
                let declaration = self.output.anonymous_struct(*struct_).ok_or_else(|| {
                    HirVerificationError::new("anonymous struct construction names invalid type")
                })?;
                let expected: Vec<ValueType> = match &declaration.kind {
                    HirAnonymousStructKind::Named(fields) => {
                        fields.iter().map(|field| field.ty).collect()
                    }
                    HirAnonymousStructKind::Positional(fields) => fields.to_vec(),
                };
                if fields.len() != expected.len() {
                    return Err(HirVerificationError::new(
                        "anonymous struct construction has wrong arity",
                    ));
                }
                let mut seen = vec![false; expected.len()];
                for field in fields {
                    let index = usize::try_from(field.field).unwrap_or(usize::MAX);
                    if index >= expected.len() || std::mem::replace(&mut seen[index], true) {
                        return Err(HirVerificationError::new(
                            "anonymous struct construction has invalid field",
                        ));
                    }
                    self.expression(function, &field.value, state)?;
                    if field.value.ty != expected[index] {
                        return Err(HirVerificationError::new(
                            "anonymous struct field initializer type differs",
                        ));
                    }
                }
                ValueType::AnonymousStruct(*struct_)
            }
            HirExpressionKind::AnonymousStructProject { aggregate, field } => {
                self.expression(function, aggregate, state)?;
                let ValueType::AnonymousStruct(struct_) = aggregate.ty else {
                    return Err(HirVerificationError::new(
                        "anonymous projection receiver is not structural",
                    ));
                };
                let declaration = self.output.anonymous_struct(struct_).ok_or_else(|| {
                    HirVerificationError::new("anonymous projection names invalid type")
                })?;
                let index = usize::try_from(*field).unwrap_or(usize::MAX);
                match &declaration.kind {
                    HirAnonymousStructKind::Named(fields) => {
                        fields.get(index).map(|field| field.ty)
                    }
                    HirAnonymousStructKind::Positional(fields) => fields.get(index).copied(),
                }
                .ok_or_else(|| {
                    HirVerificationError::new("invalid anonymous struct projection field")
                })?
            }
            HirExpressionKind::ListI32 { op, operands } => {
                for operand in operands {
                    self.expression(function, operand, state)?;
                }
                let (expected, result): (&[ValueType], ValueType) = match op {
                    HirListI32Op::Empty => (&[], ValueType::ListI32),
                    HirListI32Op::Length | HirListI32Op::LastOrZero => {
                        (&[ValueType::ListI32], ValueType::Int32)
                    }
                    HirListI32Op::Push => {
                        (&[ValueType::ListI32, ValueType::Int32], ValueType::ListI32)
                    }
                    HirListI32Op::WithoutLast => (&[ValueType::ListI32], ValueType::ListI32),
                };
                if operands.len() != expected.len()
                    || operands
                        .iter()
                        .zip(expected)
                        .any(|(operand, expected)| operand.ty != *expected)
                {
                    return Err(HirVerificationError::new(
                        "list operation operand contract is invalid",
                    ));
                }
                result
            }
            HirExpressionKind::String { op, operands } => {
                for operand in operands {
                    self.expression(function, operand, state)?;
                }
                let (expected, result): (&[ValueType], ValueType) = match op {
                    HirStringOp::Constant(_) => (&[], ValueType::String),
                    HirStringOp::Length => (&[ValueType::String], ValueType::Int32),
                    HirStringOp::EndsWithAscii(_) => (&[ValueType::String], ValueType::Bool),
                    HirStringOp::WithoutLastUnit => (&[ValueType::String], ValueType::String),
                };
                if operands.len() != expected.len()
                    || operands
                        .iter()
                        .zip(expected)
                        .any(|(operand, expected)| operand.ty != *expected)
                {
                    return Err(HirVerificationError::new(
                        "string operation operand contract is invalid",
                    ));
                }
                result
            }
            HirExpressionKind::Not(operand) => {
                self.expression(function, operand, state)?;
                if operand.ty != ValueType::Bool {
                    return Err(HirVerificationError::new("`!` operand is not Bool"));
                }
                ValueType::Bool
            }
            HirExpressionKind::WrappingArithmetic { left, right, .. } => {
                self.expression(function, left, state)?;
                self.expression(function, right, state)?;
                if left.ty != ValueType::Int32 || right.ty != ValueType::Int32 {
                    return Err(HirVerificationError::new(
                        "wrapping arithmetic operands are not Int32",
                    ));
                }
                ValueType::Int32
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
            HirExpressionKind::Switch(switch) => {
                self.origin(switch.origin, "switch expression")?;
                self.expression(function, &switch.scrutinee, state)?;
                let mut coverage = HirSwitchCoverage::new(
                    switch.scrutinee.ty,
                    self.enum_variant_count(switch.scrutinee.ty)?,
                );
                for (index, arm) in switch.arms.iter().enumerate() {
                    self.origin(arm.origin, "switch expression arm")?;
                    self.verify_switch_label(
                        &arm.label,
                        switch.scrutinee.ty,
                        &mut coverage,
                        index + 1 == switch.arms.len(),
                    )?;
                    self.expression(function, &arm.body, state)?;
                    if arm.body.ty != expression.ty {
                        return Err(HirVerificationError::new(
                            "switch expression arm result types differ",
                        ));
                    }
                }
                if !coverage.complete() {
                    return Err(HirVerificationError::new(
                        "switch expression is not exhaustive",
                    ));
                }
                expression.ty
            }
            HirExpressionKind::Index {
                aggregate,
                component,
            } => {
                self.expression(function, aggregate, state)?;
                let ValueType::AnonymousStruct(struct_) = aggregate.ty else {
                    return Err(HirVerificationError::new(
                        "index expression aggregate is not an anonymous struct",
                    ));
                };
                let declaration = self.output.anonymous_struct(struct_).ok_or_else(|| {
                    HirVerificationError::new("index expression has invalid anonymous struct")
                })?;
                let types: &[ValueType] = match &declaration.kind {
                    HirAnonymousStructKind::Positional(types) => types,
                    HirAnonymousStructKind::Named(_) => {
                        return Err(HirVerificationError::new(
                            "named anonymous structs use field projection, not index access",
                        ));
                    }
                };
                let index = usize::try_from(*component)
                    .map_err(|_| HirVerificationError::new("index component does not fit usize"))?;
                types.get(index).copied().ok_or_else(|| {
                    HirVerificationError::new(format!(
                        "index {index} is out of bounds for anonymous struct with {} components",
                        types.len()
                    ))
                })?
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

fn block_contains_return(block: &HirBlock) -> bool {
    block
        .statements
        .iter()
        .any(|statement| match &statement.kind {
            HirStatementKind::Return(_) => true,
            HirStatementKind::If(conditional) => {
                conditional
                    .arms
                    .iter()
                    .any(|arm| block_contains_return(&arm.body))
                    || conditional
                        .else_body
                        .as_ref()
                        .is_some_and(block_contains_return)
            }
            HirStatementKind::Run(run) => block_contains_return(&run.body),
            HirStatementKind::While(statement) => block_contains_return(&statement.body),
            HirStatementKind::Switch(switch) => switch
                .arms
                .iter()
                .any(|arm| block_contains_return(&arm.body)),
            HirStatementKind::Declaration { .. }
            | HirStatementKind::Assignment { .. }
            | HirStatementKind::Call(_)
            | HirStatementKind::External(_)
            | HirStatementKind::Destructure { .. }
            | HirStatementKind::Break
            | HirStatementKind::Continue => false,
        })
}

/// Checks every `Index`/`Match` segment's runtime value type-checks as
/// `Int32` — shared by `EntityNbtRead` and `EntityNbtWrite` self-verification.
fn verify_entity_nbt_segments_are_int32(
    segments: &[HirEntityPathSegment],
    external: SourceExternalOpId,
) -> Result<(), HirVerificationError> {
    for segment in segments {
        let index = match segment {
            HirEntityPathSegment::Index(index) => Some(index),
            HirEntityPathSegment::Match { value, .. } => Some(value),
            HirEntityPathSegment::Key(_) => None,
        };
        if let Some(index) = index {
            if index.ty != ValueType::Int32 {
                return Err(HirVerificationError::new(format!(
                    "entity-NBT path {external:?} has a non-Int32 index segment"
                )));
            }
        }
    }
    Ok(())
}

fn verify_run_scope_ids(block: &HirBlock, next: &mut usize) -> Result<(), HirVerificationError> {
    for statement in &block.statements {
        match &statement.kind {
            HirStatementKind::Run(run) => {
                let expected = SourceRunId::from_index(*next).ok_or_else(|| {
                    HirVerificationError::new("run-scope identity space exhausted")
                })?;
                if run.id != expected {
                    return Err(HirVerificationError::new(format!(
                        "run scope at source index {next} has non-dense identity {:?}",
                        run.id
                    )));
                }
                *next = (*next).saturating_add(1);
                verify_run_scope_ids(&run.body, next)?;
            }
            HirStatementKind::If(conditional) => {
                for arm in &conditional.arms {
                    verify_run_scope_ids(&arm.body, next)?;
                }
                if let Some(body) = &conditional.else_body {
                    verify_run_scope_ids(body, next)?;
                }
            }
            HirStatementKind::While(statement) => {
                verify_run_scope_ids(&statement.body, next)?;
            }
            HirStatementKind::Switch(switch) => {
                for arm in &switch.arms {
                    verify_run_scope_ids(&arm.body, next)?;
                }
            }
            HirStatementKind::Declaration { .. }
            | HirStatementKind::Assignment { .. }
            | HirStatementKind::Call(_)
            | HirStatementKind::External(_)
            | HirStatementKind::Return(_)
            | HirStatementKind::Destructure { .. }
            | HirStatementKind::Break
            | HirStatementKind::Continue => {}
        }
    }
    Ok(())
}

fn verify_external_operation_occurrences(
    functions: &[HirFunction],
    external_ops: &[HirExternalOp],
) -> Result<(), HirVerificationError> {
    let mut seen = vec![false; external_ops.len()];
    for function in functions {
        record_external_operation_occurrences(&function.body, &mut seen)?;
    }
    for (index, was_seen) in seen.into_iter().enumerate() {
        if !was_seen {
            return Err(HirVerificationError::new(format!(
                "external operation {:?} has no HIR occurrence",
                external_ops[index].id
            )));
        }
    }
    Ok(())
}

fn record_external_operation_occurrences(
    block: &HirBlock,
    seen: &mut [bool],
) -> Result<(), HirVerificationError> {
    for statement in &block.statements {
        match &statement.kind {
            HirStatementKind::External(external) => record_external(*external, seen)?,
            HirStatementKind::If(conditional) => {
                for arm in &conditional.arms {
                    record_expression_externals(&arm.condition, seen)?;
                    record_external_operation_occurrences(&arm.body, seen)?;
                }
                if let Some(body) = &conditional.else_body {
                    record_external_operation_occurrences(body, seen)?;
                }
            }
            HirStatementKind::Run(run) => {
                record_external_operation_occurrences(&run.body, seen)?;
            }
            HirStatementKind::While(statement) => {
                record_expression_externals(&statement.condition, seen)?;
                record_external_operation_occurrences(&statement.body, seen)?;
            }
            HirStatementKind::Switch(switch) => {
                record_expression_externals(&switch.scrutinee, seen)?;
                for arm in &switch.arms {
                    record_external_operation_occurrences(&arm.body, seen)?;
                }
            }
            HirStatementKind::Declaration { initializer, .. } => {
                if let Some(value) = initializer {
                    record_expression_externals(value, seen)?;
                }
            }
            HirStatementKind::Destructure { operand, .. } => {
                record_expression_externals(operand, seen)?;
            }
            HirStatementKind::Assignment { value, .. } => record_expression_externals(value, seen)?,
            HirStatementKind::Call(call) => {
                for argument in &call.arguments {
                    record_expression_externals(argument, seen)?;
                }
            }
            HirStatementKind::Return(value) => {
                if let Some(value) = value {
                    record_expression_externals(value, seen)?;
                }
            }
            HirStatementKind::Break | HirStatementKind::Continue => {}
        }
    }
    Ok(())
}

fn record_external(
    external: SourceExternalOpId,
    seen: &mut [bool],
) -> Result<(), HirVerificationError> {
    let occurrence = external
        .as_usize()
        .and_then(|index| seen.get_mut(index))
        .ok_or_else(|| {
            HirVerificationError::new(format!(
                "expression references invalid external operation {external:?}"
            ))
        })?;
    if std::mem::replace(occurrence, true) {
        return Err(HirVerificationError::new(format!(
            "external operation {external:?} has multiple HIR occurrences"
        )));
    }
    Ok(())
}

fn record_expression_externals(
    expression: &HirExpression,
    seen: &mut [bool],
) -> Result<(), HirVerificationError> {
    match &expression.kind {
        HirExpressionKind::External(external) => record_external(*external, seen),
        HirExpressionKind::Call(call) => {
            for argument in &call.arguments {
                record_expression_externals(argument, seen)?;
            }
            Ok(())
        }
        HirExpressionKind::Switch(switch) => {
            record_expression_externals(&switch.scrutinee, seen)?;
            for arm in &switch.arms {
                record_expression_externals(&arm.body, seen)?;
            }
            Ok(())
        }
        HirExpressionKind::StructConstruct { fields, .. }
        | HirExpressionKind::AnonymousStructConstruct { fields, .. } => {
            for field in fields {
                record_expression_externals(&field.value, seen)?;
            }
            Ok(())
        }
        HirExpressionKind::StructProject { aggregate, .. }
        | HirExpressionKind::AnonymousStructProject { aggregate, .. }
        | HirExpressionKind::Index { aggregate, .. }
        | HirExpressionKind::Not(aggregate) => record_expression_externals(aggregate, seen),
        HirExpressionKind::ListI32 { operands, .. }
        | HirExpressionKind::String { operands, .. } => {
            for operand in operands {
                record_expression_externals(operand, seen)?;
            }
            Ok(())
        }
        HirExpressionKind::WrappingArithmetic { left, right, .. }
        | HirExpressionKind::Compare { left, right, .. } => {
            record_expression_externals(left, seen)?;
            record_expression_externals(right, seen)
        }
        HirExpressionKind::Bool(_)
        | HirExpressionKind::Int32(_)
        | HirExpressionKind::EnumVariant { .. }
        | HirExpressionKind::Local(_) => Ok(()),
    }
}

struct VerifyState {
    active: Vec<bool>,
    assigned: Vec<bool>,
    changes: Vec<VerifyStateChange>,
}

struct HirSwitchCoverage {
    ty: ValueType,
    variants: Vec<bool>,
    intervals: BTreeMap<i32, i32>,
    has_else: bool,
}

impl HirSwitchCoverage {
    fn new(ty: ValueType, variants: usize) -> Self {
        Self {
            ty,
            variants: vec![false; variants],
            intervals: BTreeMap::new(),
            has_else: false,
        }
    }
    fn insert_variant(&mut self, index: usize) -> Result<(), HirVerificationError> {
        let slot = self
            .variants
            .get_mut(index)
            .ok_or_else(|| HirVerificationError::new("switch pattern has invalid variant"))?;
        if std::mem::replace(slot, true) {
            return Err(HirVerificationError::new("switch enum patterns overlap"));
        }
        Ok(())
    }
    fn insert_interval(&mut self, min: i32, max: i32) -> Result<(), HirVerificationError> {
        if self
            .intervals
            .range(..=min)
            .next_back()
            .is_some_and(|(_, old_max)| *old_max >= min)
            || self
                .intervals
                .range(min..)
                .next()
                .is_some_and(|(next_min, _)| *next_min <= max)
        {
            return Err(HirVerificationError::new("switch integer patterns overlap"));
        }
        self.intervals.insert(min, max);
        Ok(())
    }
    fn complete(&self) -> bool {
        if self.has_else {
            return true;
        }
        match self.ty {
            ValueType::Enum(_) => {
                !self.variants.is_empty() && self.variants.iter().all(|seen| *seen)
            }
            ValueType::Int32 => {
                let mut expected = i64::from(i32::MIN);
                for (min, max) in &self.intervals {
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
        HirBindingKind, HirEntityQueryStep, HirExpressionKind, HirExternalSemantic, HirRun,
        HirRunModifier, HirStatementKind, HirSwitchLabel, HirSwitchPatternKind, LocalId,
        SourceEnumId, SourceExternalOpId, SourceFunctionId, SourceRunId, SourceVariantId, verify,
    };
    use crate::frontend::FrontendLimits;
    use crate::frontend::check::{CheckOutput, check};
    use crate::frontend::lexer::lex;
    use crate::frontend::parser::parse;
    use crate::ir::semantic::{
        ContextRequirement, EntityKind, ForkBound, FunctionBehavior, ObservableEffect,
        StaticEntityQuery, TransitiveWork, WorldEffect,
    };
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

    fn first_run(checked: &mut super::CheckedFrontendOutput) -> &mut HirRun {
        let HirStatementKind::Run(run) = &mut checked.module.functions[0].body.statements[0].kind
        else {
            unreachable!();
        };
        run
    }

    const RUN_SOURCE: &str = r#"export fn scoped() {
        run.as(mc.entities(ArmorStand).limit(1)) |entity| {
            unsafe minecraft("say scoped");
        }
    }"#;

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
    fn verifier_rejects_corrupted_enum_and_variant_identities() {
        let source =
            "const State = enum { ready, done }; fn valid() { const state: State = .ready; }";

        let (sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let mut enum_identity = checked.unwrap();
        enum_identity.module.enums[0].id = SourceEnumId(7);
        assert!(
            verify(&enum_identity, &sources)
                .unwrap_err()
                .to_string()
                .contains("non-dense or empty")
        );

        let (sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let mut variant_identity = checked.unwrap();
        variant_identity.module.enums[0].variants[1].id = SourceVariantId(7);
        assert!(
            verify(&variant_identity, &sources)
                .unwrap_err()
                .to_string()
                .contains("variant inventory is non-dense")
        );
    }

    #[test]
    fn verifier_recomputes_switch_pattern_coverage() {
        let source = "fn valid(value: Int32) { switch (value) { 0...10 => {}, 11...20 => {}, else => {}, } }";
        let (sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let mut checked = checked.unwrap();
        let HirStatementKind::Switch(switch) =
            &mut checked.module.functions[0].body.statements[0].kind
        else {
            unreachable!("fixture has one switch statement")
        };
        let HirSwitchLabel::Patterns(patterns) = &mut switch.arms[1].label else {
            unreachable!("second arm has an integer pattern")
        };
        patterns[0].kind = HirSwitchPatternKind::IntRange { min: 5, max: 20 };

        assert!(
            verify(&checked, &sources)
                .unwrap_err()
                .to_string()
                .contains("patterns overlap")
        );
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

    #[test]
    fn verifier_rejects_corrupted_external_inventory_and_statement_links() {
        let source = r#"fn raw() { unsafe minecraft("say valid"); }"#;

        let (sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let mut non_dense = checked.unwrap();
        non_dense.module.external_ops[0].id = SourceExternalOpId(7);
        assert!(
            verify(&non_dense, &sources)
                .unwrap_err()
                .to_string()
                .contains("non-dense identity")
        );

        let (sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let mut malformed = checked.unwrap();
        let HirExternalSemantic::UnsafeMinecraftCommand { command, .. } =
            &mut malformed.module.external_ops[0].semantic
        else {
            unreachable!("fixture constructs one unsafe operation")
        };
        *command = "say bad\nline".into();
        assert!(
            verify(&malformed, &sources)
                .unwrap_err()
                .to_string()
                .contains("invalid physical shape")
        );

        let (sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let mut dangling = checked.unwrap();
        dangling.module.functions[0].body.statements[0].kind =
            HirStatementKind::External(SourceExternalOpId(9));
        assert!(
            verify(&dangling, &sources)
                .unwrap_err()
                .to_string()
                .contains("invalid external operation")
        );
    }

    #[test]
    fn verifier_rejects_duplicate_external_operation_occurrence() {
        let (sources, output) = checked_text(r#"fn raw() { unsafe minecraft("say valid"); }"#);
        let (checked, _) = output.into_parts();
        let mut checked = checked.unwrap();
        let statement = checked.module.functions[0].body.statements[0].clone();
        checked.module.functions[0].body.statements =
            vec![statement.clone(), statement].into_boxed_slice();

        let error = verify(&checked, &sources).unwrap_err();
        assert!(error.to_string().contains("multiple HIR occurrences"));
    }

    #[test]
    fn verifier_rejects_missing_external_operation_occurrence() {
        let (sources, output) = checked_text(r#"fn raw() { unsafe minecraft("say valid"); }"#);
        let (checked, _) = output.into_parts();
        let mut checked = checked.unwrap();
        checked.module.functions[0].body.statements = Box::new([]);

        let error = verify(&checked, &sources).unwrap_err();
        assert!(error.to_string().contains("no HIR occurrence"));
    }

    #[test]
    fn verifier_rejects_implicit_outer_local_capture_in_run_body() {
        let source = r"
            export fn scoped() {
                var outer: Int32 = 1;
                run.as(mc.entities(ArmorStand).limit(1)) {
                    var inner: Int32 = 2;
                }
            }
        ";
        let (sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let mut checked = checked.unwrap();
        let HirStatementKind::Run(run) = &mut checked.module.functions[0].body.statements[1].kind
        else {
            unreachable!();
        };
        let HirStatementKind::Declaration {
            initializer: Some(initializer),
            ..
        } = &mut run.body.statements[0].kind
        else {
            unreachable!();
        };
        initializer.kind = HirExpressionKind::Local(LocalId(0));

        let error = verify(&checked, &sources).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("referenced outside its active scope")
        );
    }

    #[test]
    fn verifier_rechecks_run_identity_and_ordered_context_transfer() {
        let (sources, output) = checked_text(RUN_SOURCE);
        let (checked, _) = output.into_parts();
        let mut non_dense = checked.unwrap();
        first_run(&mut non_dense).id = SourceRunId(7);
        assert!(
            verify(&non_dense, &sources)
                .unwrap_err()
                .to_string()
                .contains("non-dense identity")
        );

        let repeated_source = r"export fn scoped() {
            run.as(mc.entities(ArmorStand).limit(1))
                .as(mc.entities(ArmorStand).limit(1)) |entity| {}
        }";
        let (sources, output) = checked_text(repeated_source);
        let (checked, _) = output.into_parts();
        let mut repeated = checked.unwrap();
        let run = first_run(&mut repeated);
        assert_eq!(run.modifiers.len(), 2);
        let established = run.resulting_context.executor().established().unwrap();
        assert_eq!(established.1.modifier_index, 1);

        run.resulting_context = crate::ir::semantic::ExecutionContext::function_entry();
        assert!(
            verify(&repeated, &sources)
                .unwrap_err()
                .to_string()
                .contains("ordered modifier transfer")
        );
    }

    #[test]
    fn verifier_rechecks_nested_identity_and_inherited_context_provenance() {
        let source = r"export fn nested() {
            run.as(mc.entities(ArmorStand).limit(1)) |outer| {
                run |inherited| {}
            }
        }";
        let (sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let mut non_dense = checked.unwrap();
        let outer = first_run(&mut non_dense);
        let HirStatementKind::Run(inner) = &mut outer.body.statements[0].kind else {
            unreachable!()
        };
        inner.id = SourceRunId(0);
        assert!(
            verify(&non_dense, &sources)
                .unwrap_err()
                .to_string()
                .contains("non-dense identity")
        );

        let (sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let mut wrong_context = checked.unwrap();
        let outer = first_run(&mut wrong_context);
        let HirStatementKind::Run(inner) = &mut outer.body.statements[0].kind else {
            unreachable!()
        };
        inner.resulting_context = crate::ir::semantic::ExecutionContext::function_entry();
        assert!(
            verify(&wrong_context, &sources)
                .unwrap_err()
                .to_string()
                .contains("ordered modifier transfer")
        );
    }

    #[test]
    fn verifier_replays_query_steps_and_checks_their_provenance() {
        let (sources, output) = checked_text(RUN_SOURCE);
        let (checked, _) = output.into_parts();
        let mut reordered = checked.unwrap();
        let run = first_run(&mut reordered);
        let HirRunModifier::As { query, .. } = &mut run.modifiers[0] else {
            panic!("expected as")
        };
        query.steps.swap(0, 1);
        assert!(
            verify(&reordered, &sources)
                .unwrap_err()
                .to_string()
                .contains("does not begin with exactly one entities root")
        );

        let (sources, output) = checked_text(RUN_SOURCE);
        let (checked, _) = output.into_parts();
        let mut mismatched = checked.unwrap();
        let run = first_run(&mut mismatched);
        let HirRunModifier::As { query, .. } = &mut run.modifiers[0] else {
            panic!("expected as")
        };
        query.semantic = StaticEntityQuery::entities(EntityKind::ArmorStand);
        assert!(
            verify(&mismatched, &sources)
                .unwrap_err()
                .to_string()
                .contains("do not reproduce its canonical semantics")
        );

        let (sources, output) = checked_text(RUN_SOURCE);
        let (checked, _) = output.into_parts();
        let mut foreign_origin = checked.unwrap();
        let run = first_run(&mut foreign_origin);
        let HirRunModifier::As { query, .. } = &mut run.modifiers[0] else {
            panic!("expected as")
        };
        let HirEntityQueryStep::Limit { value_origin, .. } = &mut query.steps[1] else {
            unreachable!();
        };
        *value_origin = OriginId::UNKNOWN;
        assert!(
            verify(&foreign_origin, &sources)
                .unwrap_err()
                .to_string()
                .contains("invalid or unresolvable provenance")
        );
    }

    #[test]
    fn verifier_rechecks_executor_capture_proof_and_void_region() {
        let (sources, output) = checked_text(RUN_SOURCE);
        let (checked, _) = output.into_parts();
        let mut missing_executor = checked.unwrap();
        let run = first_run(&mut missing_executor);
        run.modifiers = Box::new([]);
        run.resulting_context = crate::ir::semantic::ExecutionContext::function_entry();
        assert!(
            verify(&missing_executor, &sources)
                .unwrap_err()
                .to_string()
                .contains("no modifier establishes")
        );

        let (sources, output) = checked_text(RUN_SOURCE);
        let (checked, _) = output.into_parts();
        let mut stale_proof = checked.unwrap();
        let capture = first_run(&mut stale_proof).capture.as_mut().unwrap();
        capture.proof.modifier_index = usize::MAX;
        assert!(
            verify(&stale_proof, &sources)
                .unwrap_err()
                .to_string()
                .contains("exact proof")
        );

        let (sources, output) = checked_text(RUN_SOURCE);
        let (checked, _) = output.into_parts();
        let mut returning = checked.unwrap();
        first_run(&mut returning).body.statements[0].kind = HirStatementKind::Return(None);
        assert!(
            verify(&returning, &sources)
                .unwrap_err()
                .to_string()
                .contains("contains a function return")
        );
    }

    #[test]
    fn checked_output_exposes_transitive_behavior_summaries() {
        let source = r#"
            fn empty() {}
            fn scalar() { var value: Int32 = 1; }
            fn raw() { unsafe minecraft("say raw"); }
            fn caller() { raw(); }
            fn first() { second(); }
            fn second() { first(); }
        "#;
        let (_sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let checked = checked.unwrap();

        assert_eq!(
            checked
                .function_behavior(SourceFunctionId::from_index(0).unwrap())
                .unwrap()
                .transitive_work(),
            TransitiveWork::Zero
        );
        assert_eq!(
            checked
                .function_behavior(SourceFunctionId::from_index(1).unwrap())
                .unwrap()
                .transitive_work(),
            TransitiveWork::Finite
        );
        for index in [2, 3] {
            let behavior = checked
                .function_behavior(SourceFunctionId::from_index(index).unwrap())
                .unwrap();
            assert_eq!(behavior.world_effect(), WorldEffect::Unknown);
            assert_eq!(behavior.observable_effect(), ObservableEffect::Unknown);
            assert!(behavior.contains_unsafe_unknown());
        }
        for index in [4, 5] {
            assert_eq!(
                checked
                    .function_behavior(SourceFunctionId::from_index(index).unwrap())
                    .unwrap()
                    .transitive_work(),
                TransitiveWork::NoFiniteUpperBound
            );
        }
    }

    #[test]
    fn run_behavior_retains_query_context_and_fork_facts() {
        let source = r"
            fn scoped() {
                run.as(mc.entities(ArmorStand).limit(1)) {}
            }
        ";
        let (_sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let behavior = checked
            .unwrap()
            .function_behavior(SourceFunctionId::from_index(0).unwrap())
            .unwrap();
        let context = behavior.required_ambient_context();

        assert_eq!(context.executor(), ContextRequirement::None);
        assert_eq!(context.position(), ContextRequirement::Required(()));
        assert_eq!(context.dimension(), ContextRequirement::Required(()));
        assert_eq!(context.rotation(), ContextRequirement::None);
        assert_eq!(context.anchor(), ContextRequirement::None);
        assert_eq!(behavior.world_effect(), WorldEffect::Read);
        assert_eq!(behavior.observable_effect(), ObservableEffect::None);
        assert_eq!(behavior.fork_bound(), ForkBound::finite(1).unwrap());
        assert_eq!(behavior.transitive_work(), TransitiveWork::Finite);
    }

    #[test]
    fn run_behavior_composes_prefixes_without_hiding_unbounded_queries() {
        let source = r"
            fn bounded() {
                run.as(mc.entities(ArmorStand).limit(2))
                    .as(mc.entities(ArmorStand).limit(3)) {}
            }
            fn unbounded() {
                run.as(mc.entities(ArmorStand)) {}
            }
            fn nested() {
                run.as(mc.entities(ArmorStand).limit(2)) {
                    run.as(mc.entities(ArmorStand).limit(3)) {}
                }
            }
        ";
        let (_sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let checked = checked.unwrap();

        let bounded = checked
            .function_behavior(SourceFunctionId::from_index(0).unwrap())
            .unwrap();
        assert_eq!(bounded.fork_bound(), ForkBound::finite(6).unwrap());
        assert_eq!(bounded.transitive_work(), TransitiveWork::Finite);

        let unbounded = checked
            .function_behavior(SourceFunctionId::from_index(1).unwrap())
            .unwrap();
        assert_eq!(unbounded.fork_bound(), ForkBound::NoFiniteUpperBound);
        assert_eq!(
            unbounded.transitive_work(),
            TransitiveWork::NoFiniteUpperBound
        );

        let nested = checked
            .function_behavior(SourceFunctionId::from_index(2).unwrap())
            .unwrap();
        assert_eq!(nested.fork_bound(), ForkBound::finite(3).unwrap());
    }

    #[test]
    fn outer_as_discharges_unsafe_executor_need_through_identity_run() {
        let source = r#"
            fn scoped() {
                run.as(mc.entities(ArmorStand).limit(1)) {
                    run {
                        unsafe minecraft("say scoped");
                    }
                }
            }
            fn ambient() {
                run {
                    unsafe minecraft("say ambient");
                }
            }
        "#;
        let (_sources, output) = checked_text(source);
        let (checked, _) = output.into_parts();
        let checked = checked.unwrap();

        let scoped = checked
            .function_behavior(SourceFunctionId::from_index(0).unwrap())
            .unwrap();
        assert_eq!(
            scoped.required_ambient_context().executor(),
            ContextRequirement::None
        );
        assert!(scoped.contains_unsafe_unknown());
        assert_eq!(scoped.observable_effect(), ObservableEffect::Unknown);

        let ambient = checked
            .function_behavior(SourceFunctionId::from_index(1).unwrap())
            .unwrap();
        assert_eq!(
            ambient.required_ambient_context().executor(),
            ContextRequirement::Unknown
        );
        assert!(ambient.contains_unsafe_unknown());
        assert_eq!(ambient.observable_effect(), ObservableEffect::Unknown);
    }

    #[test]
    fn unreachable_unsafe_suffix_does_not_contaminate_behavior() {
        let (_sources, output) = checked_text(
            r#"fn stopped() {
                return;
                unsafe minecraft("say unreachable");
            }"#,
        );
        let (checked, _) = output.into_parts();
        let behavior = checked
            .unwrap()
            .function_behavior(SourceFunctionId::from_index(0).unwrap())
            .unwrap();

        assert!(!behavior.contains_unsafe_unknown());
        assert_eq!(behavior.world_effect(), WorldEffect::None);
        assert_eq!(behavior.observable_effect(), ObservableEffect::None);
        assert_ne!(behavior.transitive_work(), TransitiveWork::Unknown);
    }

    #[test]
    fn verifier_recomputes_the_dense_behavior_table() {
        let (sources, output) = checked_text("fn valid() { var value: Int32 = 1; }");
        let (checked, _) = output.into_parts();
        let mut truncated = checked.unwrap();
        truncated.module.behaviors = Box::new([]);
        assert!(
            verify(&truncated, &sources)
                .unwrap_err()
                .to_string()
                .contains("behavior summaries")
        );

        let (sources, output) = checked_text("fn valid() { var value: Int32 = 1; }");
        let (checked, _) = output.into_parts();
        let mut corrupted = checked.unwrap();
        corrupted.module.behaviors[0] = FunctionBehavior::NONE;
        assert!(
            verify(&corrupted, &sources)
                .unwrap_err()
                .to_string()
                .contains("does not match HIR inference")
        );

        let (sources, output) = checked_text(r#"fn raw() { unsafe minecraft("say raw"); }"#);
        let (checked, _) = output.into_parts();
        let mut corrupted = checked.unwrap();
        let inferred = corrupted.module.behaviors[0];
        corrupted.module.behaviors[0] = FunctionBehavior::new(
            inferred.required_ambient_context(),
            inferred.world_effect(),
            ObservableEffect::None,
            inferred.fork_bound(),
            inferred.transitive_work(),
            inferred.contains_unsafe_unknown(),
        );
        assert!(
            verify(&corrupted, &sources)
                .unwrap_err()
                .to_string()
                .contains("does not match HIR inference")
        );
    }
}
