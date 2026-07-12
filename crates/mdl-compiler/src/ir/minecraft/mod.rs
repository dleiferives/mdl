//! Typed Minecraft target syntax.

mod builder;
mod command;
mod contract;
mod data;
mod dump;
mod execute;
mod names;
mod nbt;
mod number;
mod program;
mod render;
mod score;
mod selector;
mod verify;

pub use builder::{BuildError, FunctionBodyBuilder, FunctionTagBuilder, MinecraftProgramBuilder};
pub use command::{
    CommandDepthError, CommandKind, CommandNode, FunctionCall, MAX_COMMAND_DEPTH, RawCommandError,
    ReturnCommand, UnsafeRawCommand,
};
pub use contract::{
    CommandContract, ContextMask, ContextSummary, EffectCategories, EffectSummary, ForkClass,
};
pub use data::{DataCommand, DataModifyMode, DataSource};
pub use dump::MinecraftDebugDumper;
pub use execute::{
    Condition, ExecuteCommand, ExecuteModifier, ExecuteModifierKind, ExecuteModifiers,
    ScoreComparison, StorageNumericType, StoreChannel, StoreDestination,
};
pub use names::{
    DimensionId, FunctionResourceId, FunctionTagResourceId, NameError, NameErrorReason, NameKind,
    Namespace, PackNamespace, PackPath, PackResourcePath, ResourcePath, StorageId,
};
pub use nbt::{
    EmptyNbtPath, MAX_NBT_DEPTH, NbtBuildError, NbtKey, NbtPath, NbtPathKey, NbtPathKeyError,
    NbtPathKeyErrorReason, NbtPathSegment, NbtValue, NbtValueRef, StoragePath,
};
pub use number::{FiniteF32, FiniteF64, FloatWidth, NonFiniteFloat, NonFiniteKind};
pub use program::{
    CallableRef, CommandId, ExternalCallableRef, ExternalTagRequirement, FunctionBody, FunctionTag,
    FunctionTagEntry, FunctionTagEntryKind, FunctionTagId, FunctionTagMerge, InternalCallableRef,
    McFunction, McFunctionId, MinecraftProgram,
};
pub use score::{
    BackwardsScoreRange, FakeScoreHolder, NegativeScoreAmount, NonNegativeI32, ObjectiveName,
    ScoreCommand, ScoreHolders, ScoreNameError, ScoreNameErrorReason, ScoreNameKind,
    ScoreOperation, ScoreRange, ScoreRangeKind, ScoreRef, ScoreSelection, SingleScoreHolder,
};
pub use selector::{AtMostOneSelector, Cardinality, Selector, UnboundedSelector};
pub use verify::verify_program;

pub(crate) use render::{RenderError, render_function};
