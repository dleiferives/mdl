//! Typed Minecraft target syntax.

mod builder;
mod command;
mod contract;
mod data;
mod dump;
mod execute;
mod macro_command;
mod names;
mod nbt;
mod number;
mod program;
mod render;
mod say;
mod score;
mod selector;
mod spatial;
mod verify;

pub use builder::{BuildError, FunctionBodyBuilder, FunctionTagBuilder, MinecraftProgramBuilder};
pub use command::{
    CommandDepthError, CommandKind, CommandNode, FunctionCall, FunctionWithStorage,
    MAX_COMMAND_DEPTH, RawCommandError, ReturnCommand, UnsafeRawCommand,
};
pub use contract::{
    CommandContract, ContextMask, ContextSummary, EffectCategories, EffectSummary, ForkClass,
    NativeCommandOutcome,
};
pub use data::{DataCommand, DataModifyMode, DataSource};
pub use dump::MinecraftDebugDumper;
pub use execute::{
    Condition, ExecuteCommand, ExecuteModifier, ExecuteModifierKind, ExecuteModifiers,
    ScoreComparison, StorageNumericType, StoreChannel, StoreDestination,
};
pub use macro_command::{
    MacroArguments, MacroArgumentsError, MacroArgumentsErrorReason, MacroCommand, MacroKeyError,
    MacroLine, MacroSegment, MacroSlot, MacroVariable, MacroVariableId,
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
pub use say::{MAX_SAY_MESSAGE_UTF16_UNITS, SayCommand, SayMessage, SayMessageError};
pub use score::{
    BackwardsScoreRange, FakeScoreHolder, NegativeScoreAmount, NonNegativeI32, ObjectiveName,
    ScoreCommand, ScoreHolders, ScoreNameError, ScoreNameErrorReason, ScoreNameKind,
    ScoreOperation, ScoreRange, ScoreRangeKind, ScoreRef, ScoreSelection, SingleScoreHolder,
};
pub use selector::{
    AtMostOneSelector, Cardinality, EntitySelector, EntitySelectorError, SelectedEntityKind,
    Selector, UnboundedSelector,
};
pub use spatial::{
    JavaDecimal, JavaDecimalError, TargetAnchor, TargetAxes, TargetLocalPosition, TargetPosition,
    TargetRotation, TargetRotationAxis, TargetWorldAxis, TargetWorldPosition, TeleportCommand,
};
pub use verify::verify_program;

pub(crate) use render::{RenderError, render_function};
