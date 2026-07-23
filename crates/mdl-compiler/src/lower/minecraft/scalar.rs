use crate::diagnostic::Diagnostics;
use crate::ir::core::{CoreOp, CoreType, I32Predicate, Operand};
use crate::ir::minecraft::{
    CommandKind, Condition, DataCommand, DataModifyMode, DataSource, ExecuteCommand,
    ExecuteModifier, ExecuteModifierKind, ExecuteModifiers, FiniteF64, NbtKey, NbtPath,
    NbtPathSegment, NbtValue, ScoreCommand, ScoreComparison, ScoreHolders, ScoreOperation,
    ScoreRange, ScoreRef, ScoreSelection, SingleScoreHolder, StorageNumericType, StoragePath,
    StoreChannel, StoreDestination,
};
use crate::source::OriginId;

use super::construct::{FunctionLoweringCx, command, invariant_diagnostics};
use super::plan::HomeId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScalarLowering {
    Lowered,
    Call,
}

/// Ordered physical result slots required by one fixed scalar recipe.
///
/// This target contract deliberately lives beside emission rather than on semantic
/// Core operations. The production plan verifier interprets recipes independently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScalarOutputContract {
    result_types: &'static [CoreType],
}

/// Exhaustive physical access timing for one fixed Minecraft scalar recipe.
///
/// Each result names the sole operand position whose home it may reuse, if any.
/// This is target timing rather than a semantic property of `CoreOp`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScalarAccessContract {
    output: ScalarOutputContract,
    reusable_operands: &'static [Option<usize>],
}

impl ScalarAccessContract {
    pub(super) const fn output(self) -> ScalarOutputContract {
        self.output
    }

    pub(super) const fn reusable_operand(self, result_index: usize) -> Option<usize> {
        if result_index < self.reusable_operands.len() {
            self.reusable_operands[result_index]
        } else {
            None
        }
    }
}

impl ScalarOutputContract {
    pub(super) const fn result_types(self) -> &'static [CoreType] {
        self.result_types
    }
}

const BOOL_OUTPUT: &[CoreType] = &[CoreType::Bool];
const I32_OUTPUT: &[CoreType] = &[CoreType::I32];
const OVERFLOWING_ADD_OUTPUTS: &[CoreType] = &[CoreType::I32, CoreType::Bool];
const LIST_I32_OUTPUT: &[CoreType] = &[CoreType::ListI32];
const STRING_OUTPUT: &[CoreType] = &[CoreType::String];
const ONE_NO_REUSE: &[Option<usize>] = &[None];
const TWO_NO_REUSE: &[Option<usize>] = &[None, None];
const WRAPPING_ADD_REUSE: &[Option<usize>] = &[Some(0)];
const WRAPPING_SUB_REUSE: &[Option<usize>] = &[Some(0)];

pub(super) const fn scalar_access_contract(operation: &CoreOp) -> Option<ScalarAccessContract> {
    let (result_types, reusable_operands) = match operation {
        CoreOp::BoolConstant(_)
        | CoreOp::I32Compare(_)
        | CoreOp::I32InClosedRange(_)
        | CoreOp::BoolNot
        | CoreOp::StringEndsWithAscii(_) => (BOOL_OUTPUT, ONE_NO_REUSE),
        CoreOp::I32Constant(_)
        | CoreOp::ListI32Length
        | CoreOp::ListI32LastOrZero
        | CoreOp::StringLength => (I32_OUTPUT, ONE_NO_REUSE),
        CoreOp::I32AddWrapping => (I32_OUTPUT, WRAPPING_ADD_REUSE),
        CoreOp::I32SubWrapping => (I32_OUTPUT, WRAPPING_SUB_REUSE),
        CoreOp::I32AddOverflowing => (OVERFLOWING_ADD_OUTPUTS, TWO_NO_REUSE),
        CoreOp::ListI32Empty | CoreOp::ListI32Push | CoreOp::ListI32WithoutLast => {
            (LIST_I32_OUTPUT, ONE_NO_REUSE)
        }
        CoreOp::StringConstant(_) | CoreOp::StringWithoutLastUnit => (STRING_OUTPUT, ONE_NO_REUSE),
        CoreOp::Call(_) | CoreOp::External(_) => return None,
    };
    Some(ScalarAccessContract {
        output: ScalarOutputContract { result_types },
        reusable_operands,
    })
}

pub(super) const fn scalar_output_contract(operation: &CoreOp) -> Option<ScalarOutputContract> {
    match scalar_access_contract(operation) {
        Some(contract) => Some(contract.output()),
        None => None,
    }
}

pub(crate) fn lower_scalar_operation(
    context: &mut FunctionLoweringCx<'_, '_>,
    operation: &CoreOp,
    operands: &[HomeId],
    results: &[HomeId],
    origin: OriginId,
) -> Result<ScalarLowering, Diagnostics> {
    match operation {
        CoreOp::BoolConstant(value) => {
            let ([], [result]) = (operands, results) else {
                return Err(invalid_scalar_shape(operation, origin));
            };
            lower_bool_constant(context, *result, *value, origin)?;
        }
        CoreOp::I32Constant(value) => {
            let ([], [result]) = (operands, results) else {
                return Err(invalid_scalar_shape(operation, origin));
            };
            lower_i32_constant(context, *result, *value, origin)?;
        }
        CoreOp::I32AddWrapping => {
            let ([left, right], [result]) = (operands, results) else {
                return Err(invalid_scalar_shape(operation, origin));
            };
            lower_i32_add_wrapping(context, *result, *left, *right, origin)?;
        }
        CoreOp::I32SubWrapping => {
            let ([left, right], [result]) = (operands, results) else {
                return Err(invalid_scalar_shape(operation, origin));
            };
            lower_i32_sub_wrapping(context, *result, *left, *right, origin)?;
        }
        CoreOp::I32AddOverflowing => {
            let ([left, right], [sum, overflowed]) = (operands, results) else {
                return Err(invalid_scalar_shape(operation, origin));
            };
            lower_i32_add_overflowing(context, *sum, *overflowed, *left, *right, origin)?;
        }
        CoreOp::I32Compare(predicate) => {
            let ([left, right], [result]) = (operands, results) else {
                return Err(invalid_scalar_shape(operation, origin));
            };
            lower_i32_compare(context, *result, *predicate, *left, *right, origin)?;
        }
        CoreOp::I32InClosedRange(range) => {
            let ([value], [result]) = (operands, results) else {
                return Err(invalid_scalar_shape(operation, origin));
            };
            lower_i32_in_closed_range(context, *result, *value, *range, origin)?;
        }
        CoreOp::BoolNot => {
            let ([operand], [result]) = (operands, results) else {
                return Err(invalid_scalar_shape(operation, origin));
            };
            lower_bool_not(context, *result, *operand, origin)?;
        }
        CoreOp::ListI32Empty
        | CoreOp::ListI32Length
        | CoreOp::ListI32Push
        | CoreOp::ListI32LastOrZero
        | CoreOp::ListI32WithoutLast => {
            lower_list_i32_operation(context, operation, operands, results, origin)?;
        }
        CoreOp::StringConstant(_)
        | CoreOp::StringLength
        | CoreOp::StringEndsWithAscii(_)
        | CoreOp::StringWithoutLastUnit => {
            lower_string_operation(context, operation, operands, results, origin)?;
        }
        CoreOp::Call(_) => return Ok(ScalarLowering::Call),
        CoreOp::External(_) => return Err(invalid_scalar_shape(operation, origin)),
    }
    Ok(ScalarLowering::Lowered)
}

/// Copies one semantic home using its type-selected physical representation.
pub(super) fn copy_home(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    source: HomeId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let destination_type = context.plan().home_type(destination);
    let source_type = context.plan().home_type(source);
    let nbt_type = [destination_type, source_type]
        .into_iter()
        .flatten()
        .find(|ty| matches!(ty, CoreType::ListI32 | CoreType::String));
    if let Some(nbt_type) = nbt_type {
        if destination_type.is_some_and(|ty| ty != nbt_type)
            || source_type.is_some_and(|ty| ty != nbt_type)
        {
            return Err(invariant_diagnostics(
                "NBT transfer mixes incompatible physical home types",
                origin,
            ));
        }
        let storage = |home| match nbt_type {
            CoreType::ListI32 => context.plan().list_storage(home),
            CoreType::String => context.plan().string_storage(home),
            CoreType::Bool | CoreType::I32 => None,
        };
        let target = storage(destination).ok_or_else(|| {
            invariant_diagnostics("NBT transfer destination has no storage path", origin)
        })?;
        let source = storage(source).ok_or_else(|| {
            invariant_diagnostics("NBT transfer source has no storage path", origin)
        })?;
        context.push(command(
            CommandKind::Data(DataCommand::Modify {
                target,
                mode: DataModifyMode::Set,
                source: DataSource::From(source),
            }),
            origin,
        )?)
    } else {
        score_operation(context, destination, ScoreOperation::Assign, source, origin)
    }
}

fn lower_string_operation(
    context: &mut FunctionLoweringCx<'_, '_>,
    operation: &CoreOp,
    operands: &[HomeId],
    results: &[HomeId],
    origin: OriginId,
) -> Result<(), Diagnostics> {
    match (operation, operands, results) {
        (CoreOp::StringConstant(value), [], [result]) => {
            context.require_type(*result, CoreType::String)?;
            context.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: string_path(context, *result, origin)?,
                    mode: DataModifyMode::Set,
                    source: DataSource::Value(NbtValue::string(value)),
                }),
                origin,
            )?)
        }
        (CoreOp::StringLength, [value], [result]) => {
            context.require_type(*value, CoreType::String)?;
            context.require_type(*result, CoreType::I32)?;
            store_data_result_in_score(
                context,
                *result,
                DataCommand::Get {
                    source: string_path(context, *value, origin)?,
                    scale: None,
                },
                origin,
            )
        }
        (CoreOp::StringWithoutLastUnit, [value], [result]) => {
            context.require_type(*value, CoreType::String)?;
            context.require_type(*result, CoreType::String)?;
            copy_home(context, *result, *value, origin)?;
            context.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: string_path(context, *result, origin)?,
                    mode: DataModifyMode::Set,
                    source: DataSource::StringSlice {
                        source: string_path(context, *value, origin)?,
                        start: 0,
                        end: Some(-1),
                    },
                }),
                origin,
            )?)
        }
        (CoreOp::StringEndsWithAscii(ascii), [value], [result]) if ascii.is_ascii() => {
            context.require_type(*value, CoreType::String)?;
            context.require_type(*result, CoreType::Bool)?;
            let scratch = context.plan().string_unit_scratch();
            context.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: scratch.clone(),
                    mode: DataModifyMode::Set,
                    source: DataSource::Value(NbtValue::string("")),
                }),
                origin,
            )?)?;
            context.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: scratch.clone(),
                    mode: DataModifyMode::Set,
                    source: DataSource::StringSlice {
                        source: string_path(context, *value, origin)?,
                        start: -1,
                        end: None,
                    },
                }),
                origin,
            )?)?;
            lower_bool_constant(context, *result, false, origin)?;
            let pattern = NbtValue::compound(vec![(
                NbtKey::new("string_unit_scratch"),
                NbtValue::string(&char::from(*ascii).to_string()),
            )])
            .map_err(|error| {
                invariant_diagnostics(format!("string pattern is invalid: {error}"), origin)
            })?;
            conditional_set(
                context,
                [(
                    true,
                    Condition::DataMatches(scratch.storage().clone(), pattern),
                )],
                *result,
                1,
                origin,
            )
        }
        _ => Err(invalid_scalar_shape(operation, origin)),
    }
}

fn string_path(
    context: &FunctionLoweringCx<'_, '_>,
    home: HomeId,
    origin: OriginId,
) -> Result<StoragePath, Diagnostics> {
    context
        .plan()
        .string_storage(home)
        .ok_or_else(|| invariant_diagnostics("string home has no physical storage path", origin))
}

fn lower_list_i32_operation(
    context: &mut FunctionLoweringCx<'_, '_>,
    operation: &CoreOp,
    operands: &[HomeId],
    results: &[HomeId],
    origin: OriginId,
) -> Result<(), Diagnostics> {
    match (operation, operands, results) {
        (CoreOp::ListI32Empty, [], [result]) => {
            context.require_type(*result, CoreType::ListI32)?;
            let empty = NbtValue::list(vec![]).map_err(|error| {
                invariant_diagnostics(format!("empty list literal is invalid: {error}"), origin)
            })?;
            context.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: list_path(context, *result, origin)?,
                    mode: DataModifyMode::Set,
                    source: DataSource::Value(empty),
                }),
                origin,
            )?)
        }
        (CoreOp::ListI32Length, [list], [result]) => {
            context.require_type(*list, CoreType::ListI32)?;
            context.require_type(*result, CoreType::I32)?;
            store_data_result_in_score(
                context,
                *result,
                DataCommand::Get {
                    source: list_path(context, *list, origin)?,
                    scale: None,
                },
                origin,
            )
        }
        (CoreOp::ListI32Push, [list, element], [result]) => {
            context.require_type(*list, CoreType::ListI32)?;
            context.require_type(*element, CoreType::I32)?;
            context.require_type(*result, CoreType::ListI32)?;
            copy_home(context, *result, *list, origin)?;
            let get = command(
                CommandKind::Score(ScoreCommand::PlayersGet {
                    score: context.score(*element)?,
                }),
                origin,
            )?;
            let store = ExecuteModifier::new(
                ExecuteModifierKind::Store(
                    StoreChannel::Result,
                    StoreDestination::Storage {
                        target: context.plan().list_i32_scratch(),
                        numeric_type: StorageNumericType::Int,
                        scale: FiniteF64::new(1.0).expect("one is finite"),
                    },
                ),
                origin,
            );
            context.push(command(
                CommandKind::Execute(ExecuteCommand::new(
                    ExecuteModifiers::new(store, vec![]),
                    get,
                )),
                origin,
            )?)?;
            context.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: list_path(context, *result, origin)?,
                    mode: DataModifyMode::Append,
                    source: DataSource::From(context.plan().list_i32_scratch()),
                }),
                origin,
            )?)
        }
        (CoreOp::ListI32LastOrZero, [list], [result]) => {
            context.require_type(*list, CoreType::ListI32)?;
            context.require_type(*result, CoreType::I32)?;
            lower_i32_constant(context, *result, 0, origin)?;
            store_data_result_in_score_if_present(
                context,
                *result,
                list_element_path(context, *list, -1, origin)?,
                origin,
            )
        }
        (CoreOp::ListI32WithoutLast, [list], [result]) => {
            context.require_type(*list, CoreType::ListI32)?;
            context.require_type(*result, CoreType::ListI32)?;
            copy_home(context, *result, *list, origin)?;
            context.push(command(
                CommandKind::Data(DataCommand::Remove {
                    target: list_element_path(context, *result, -1, origin)?,
                }),
                origin,
            )?)
        }
        _ => Err(invalid_scalar_shape(operation, origin)),
    }
}

fn list_path(
    context: &FunctionLoweringCx<'_, '_>,
    home: HomeId,
    origin: OriginId,
) -> Result<StoragePath, Diagnostics> {
    context
        .plan()
        .list_storage(home)
        .ok_or_else(|| invariant_diagnostics("list home has no physical storage path", origin))
}

fn list_element_path(
    context: &FunctionLoweringCx<'_, '_>,
    home: HomeId,
    index: i32,
    origin: OriginId,
) -> Result<StoragePath, Diagnostics> {
    let base = list_path(context, home, origin)?;
    let mut segments = base.path().segments().to_vec();
    segments.push(NbtPathSegment::Index(Operand::Const(index)));
    Ok(StoragePath::new(
        base.storage().clone(),
        NbtPath::from_segments(segments).expect("list element path is nonempty"),
    ))
}

fn store_data_result_in_score(
    context: &mut FunctionLoweringCx<'_, '_>,
    result: HomeId,
    data: DataCommand,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let modifier = ExecuteModifier::new(
        ExecuteModifierKind::Store(
            StoreChannel::Result,
            StoreDestination::Score(context.score(result)?),
        ),
        origin,
    );
    let get = command(CommandKind::Data(data), origin)?;
    context.push(command(
        CommandKind::Execute(ExecuteCommand::new(
            ExecuteModifiers::new(modifier, vec![]),
            get,
        )),
        origin,
    )?)
}

/// Reads one optional numeric NBT value without depending on the result of a
/// failed `data get`. Callers initialize `result` to their semantic fallback.
fn store_data_result_in_score_if_present(
    context: &mut FunctionLoweringCx<'_, '_>,
    result: HomeId,
    source: StoragePath,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let exists = ExecuteModifier::new(
        ExecuteModifierKind::If(Condition::DataExists(source.clone())),
        origin,
    );
    let store = ExecuteModifier::new(
        ExecuteModifierKind::Store(
            StoreChannel::Result,
            StoreDestination::Score(context.score(result)?),
        ),
        origin,
    );
    let get = command(
        CommandKind::Data(DataCommand::Get {
            source,
            scale: None,
        }),
        origin,
    )?;
    context.push(command(
        CommandKind::Execute(ExecuteCommand::new(
            ExecuteModifiers::new(exists, vec![store]),
            get,
        )),
        origin,
    )?)
}

fn invalid_scalar_shape(operation: &CoreOp, origin: OriginId) -> Diagnostics {
    super::construct::invariant_diagnostics(
        format!(
            "verified {} has an invalid scalar operand/result shape",
            operation.name()
        ),
        origin,
    )
}

pub(crate) fn lower_bool_constant(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    value: bool,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    context.require_type(destination, CoreType::Bool)?;
    set_literal(context, destination, i32::from(value), origin)
}

pub(crate) fn lower_i32_constant(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    value: i32,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    context.require_type(destination, CoreType::I32)?;
    set_literal(context, destination, value, origin)
}

fn set_literal(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    value: i32,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let target = selection(&context.score(destination)?);
    context.push(command(
        CommandKind::Score(ScoreCommand::PlayersSet { target, value }),
        origin,
    )?)
}

pub(crate) fn lower_i32_add_wrapping(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    left: HomeId,
    right: HomeId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    for home in [destination, left, right] {
        context.require_type(home, CoreType::I32)?;
    }
    if destination != left {
        score_operation(context, destination, ScoreOperation::Assign, left, origin)?;
    }
    score_operation(context, destination, ScoreOperation::Add, right, origin)
}

pub(crate) fn lower_i32_sub_wrapping(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    left: HomeId,
    right: HomeId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    for home in [destination, left, right] {
        context.require_type(home, CoreType::I32)?;
    }
    if destination != left {
        score_operation(context, destination, ScoreOperation::Assign, left, origin)?;
    }
    score_operation(
        context,
        destination,
        ScoreOperation::Subtract,
        right,
        origin,
    )
}

pub(crate) fn lower_i32_add_overflowing(
    context: &mut FunctionLoweringCx<'_, '_>,
    sum: HomeId,
    overflowed: HomeId,
    left: HomeId,
    right: HomeId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    for home in [sum, left, right] {
        context.require_type(home, CoreType::I32)?;
    }
    context.require_type(overflowed, CoreType::Bool)?;
    score_operation(context, sum, ScoreOperation::Assign, left, origin)?;
    score_operation(context, sum, ScoreOperation::Add, right, origin)?;
    set_literal(context, overflowed, 0, origin)?;
    let positive_overflow = [
        Condition::ScoreMatches(context.score(left)?, ScoreRange::at_least(0)),
        Condition::ScoreMatches(context.score(right)?, ScoreRange::at_least(0)),
        Condition::ScoreMatches(context.score(sum)?, ScoreRange::at_most(-1)),
    ];
    conditional_set(
        context,
        positive_overflow
            .into_iter()
            .map(|condition| (true, condition)),
        overflowed,
        1,
        origin,
    )?;
    let negative_overflow = [
        Condition::ScoreMatches(context.score(left)?, ScoreRange::at_most(-1)),
        Condition::ScoreMatches(context.score(right)?, ScoreRange::at_most(-1)),
        Condition::ScoreMatches(context.score(sum)?, ScoreRange::at_least(0)),
    ];
    conditional_set(
        context,
        negative_overflow
            .into_iter()
            .map(|condition| (true, condition)),
        overflowed,
        1,
        origin,
    )
}

pub(crate) fn lower_i32_compare(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    predicate: I32Predicate,
    left: HomeId,
    right: HomeId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    context.require_type(destination, CoreType::Bool)?;
    context.require_type(left, CoreType::I32)?;
    context.require_type(right, CoreType::I32)?;
    set_literal(context, destination, 0, origin)?;
    let (positive, comparison) = comparison_condition(predicate);
    conditional_set(
        context,
        [(
            positive,
            Condition::ScoreCompare(context.score(left)?, comparison, context.score(right)?),
        )],
        destination,
        1,
        origin,
    )
}

pub(crate) fn lower_i32_in_closed_range(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    value: HomeId,
    range: crate::ir::core::I32ClosedRange,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    context.require_type(destination, CoreType::Bool)?;
    context.require_type(value, CoreType::I32)?;
    set_literal(context, destination, 0, origin)?;
    let score_range = ScoreRange::between(range.min(), range.max()).map_err(|_| {
        invariant_diagnostics("validated Core closed range became backwards", origin)
    })?;
    conditional_set(
        context,
        [(
            true,
            Condition::ScoreMatches(context.score(value)?, score_range),
        )],
        destination,
        1,
        origin,
    )
}

const fn comparison_condition(predicate: I32Predicate) -> (bool, ScoreComparison) {
    match predicate {
        I32Predicate::Eq => (true, ScoreComparison::Equal),
        I32Predicate::Ne => (false, ScoreComparison::Equal),
        I32Predicate::SignedLt => (true, ScoreComparison::LessThan),
        I32Predicate::SignedLe => (true, ScoreComparison::LessOrEqual),
        I32Predicate::SignedGt => (true, ScoreComparison::GreaterThan),
        I32Predicate::SignedGe => (true, ScoreComparison::GreaterOrEqual),
    }
}

pub(crate) fn lower_bool_not(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    operand: HomeId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    context.require_type(destination, CoreType::Bool)?;
    context.require_type(operand, CoreType::Bool)?;
    set_literal(context, destination, 1, origin)?;
    score_operation(
        context,
        destination,
        ScoreOperation::Subtract,
        operand,
        origin,
    )
}

pub(crate) fn score_operation(
    context: &mut FunctionLoweringCx<'_, '_>,
    destination: HomeId,
    operation: ScoreOperation,
    source: HomeId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let target = selection(&context.score(destination)?);
    let source = selection(&context.score(source)?);
    context.push(command(
        CommandKind::Score(ScoreCommand::PlayersOperation {
            target,
            op: operation,
            source,
        }),
        origin,
    )?)
}

fn conditional_set(
    context: &mut FunctionLoweringCx<'_, '_>,
    conditions: impl IntoIterator<Item = (bool, Condition)>,
    destination: HomeId,
    value: i32,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let mut modifiers = conditions.into_iter().map(|(positive, condition)| {
        ExecuteModifier::new(
            if positive {
                ExecuteModifierKind::If(condition)
            } else {
                ExecuteModifierKind::Unless(condition)
            },
            origin,
        )
    });
    let first = modifiers.next().ok_or_else(|| {
        super::construct::invariant_diagnostics(
            "conditional scalar assignment has no condition",
            origin,
        )
    })?;
    let run = command(
        CommandKind::Score(ScoreCommand::PlayersSet {
            target: selection(&context.score(destination)?),
            value,
        }),
        origin,
    )?;
    context.push(command(
        CommandKind::Execute(ExecuteCommand::new(
            ExecuteModifiers::new(first, modifiers.collect()),
            run,
        )),
        origin,
    )?)
}

fn selection(score: &ScoreRef) -> ScoreSelection {
    let holders = match score.holder() {
        SingleScoreHolder::Fake(holder) => ScoreHolders::Fake(holder.clone()),
        SingleScoreHolder::Selector(selector) => {
            ScoreHolders::Selector(crate::ir::minecraft::Selector::from(*selector))
        }
    };
    ScoreSelection::new(holders, score.objective().clone())
}

#[cfg(test)]
mod tests {
    use super::{
        ScalarLowering, lower_bool_constant, lower_bool_not, lower_i32_add_overflowing,
        lower_i32_add_wrapping, lower_i32_compare, lower_i32_constant, lower_scalar_operation,
    };
    use crate::ir::core::{
        CoreOp, CoreProgram, FunctionBuilder, I32Predicate, Terminator, TerminatorKind,
    };
    use crate::ir::minecraft::ScoreComparison;
    use crate::ir::minecraft::{ObjectiveName, PackNamespace, render_function};
    use crate::lower::minecraft::LoweringOptions;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::construct::TargetConstruction;
    use crate::lower::minecraft::plan::PlanBuilder;
    use crate::source::{Origin, OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one end-to-end golden keeps the complete scalar command order visible"
    )]
    fn constants_and_wrapping_add_lower_to_exact_two_address_commands() {
        let mut sources = SourceContext::new();
        let instruction_origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("scalar"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut body = FunctionBuilder::new(&core, &sources, function).unwrap();
        let left = body.i32_constant(i32::MIN, instruction_origin).unwrap();
        let right = body.i32_constant(-1, instruction_origin).unwrap();
        let flag = body.bool_constant(true, instruction_origin).unwrap();
        let sum = body
            .i32_add_wrapping(left, right, instruction_origin)
            .unwrap();
        let (overflow_sum, overflowed) = body
            .i32_add_overflowing(left, right, instruction_origin)
            .unwrap();
        let not_equal = body
            .i32_compare(I32Predicate::Ne, left, right, instruction_origin)
            .unwrap();
        let negated = body.bool_not(flag, instruction_origin).unwrap();
        body.terminate(Terminator::new(
            TerminatorKind::Return(vec![]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
        core.define_function(function, body.finish().unwrap())
            .unwrap();
        let analyses = SemanticInventory::new(&core).unwrap();
        let mut plan_builder = PlanBuilder::new(&core, options()).unwrap();
        plan_builder
            .physicalize(
                &core,
                &analyses,
                &crate::lower::minecraft::demand::RuntimeDemand::optimization_disabled(),
            )
            .unwrap();
        let plan = plan_builder.finish(&core, &analyses).unwrap();
        let block = core.function(function).unwrap().body().unwrap().entry();
        let planned = plan.block_function(function, block).unwrap();
        let mut construction = TargetConstruction::declare(&core, &plan).unwrap();
        construction.define_initialization(&plan).unwrap();
        let mut context = construction
            .begin_function(
                core.function(function).unwrap().body().unwrap(),
                &plan,
                planned,
            )
            .unwrap();
        lower_i32_constant(
            &mut context,
            plan.value_home(function, left).unwrap(),
            i32::MIN,
            instruction_origin,
        )
        .unwrap();
        lower_i32_constant(
            &mut context,
            plan.value_home(function, right).unwrap(),
            -1,
            instruction_origin,
        )
        .unwrap();
        lower_bool_constant(
            &mut context,
            plan.value_home(function, flag).unwrap(),
            true,
            instruction_origin,
        )
        .unwrap();
        lower_i32_add_wrapping(
            &mut context,
            plan.value_home(function, sum).unwrap(),
            plan.value_home(function, left).unwrap(),
            plan.value_home(function, right).unwrap(),
            instruction_origin,
        )
        .unwrap();
        lower_i32_add_overflowing(
            &mut context,
            plan.value_home(function, overflow_sum).unwrap(),
            plan.value_home(function, overflowed).unwrap(),
            plan.value_home(function, left).unwrap(),
            plan.value_home(function, right).unwrap(),
            instruction_origin,
        )
        .unwrap();
        lower_i32_compare(
            &mut context,
            plan.value_home(function, not_equal).unwrap(),
            I32Predicate::Ne,
            plan.value_home(function, left).unwrap(),
            plan.value_home(function, right).unwrap(),
            instruction_origin,
        )
        .unwrap();
        lower_bool_not(
            &mut context,
            plan.value_home(function, negated).unwrap(),
            plan.value_home(function, flag).unwrap(),
            instruction_origin,
        )
        .unwrap();
        assert_eq!(
            lower_scalar_operation(
                &mut context,
                &CoreOp::Call(function),
                &[],
                &[],
                instruction_origin,
            )
            .unwrap(),
            ScalarLowering::Call
        );
        assert!(
            lower_scalar_operation(
                &mut context,
                &CoreOp::I32Constant(0),
                &[],
                &[],
                instruction_origin,
            )
            .is_err()
        );
        context.finish();
        let target = construction.declarations().function(planned).unwrap();
        let program = construction.finish().unwrap();
        let function = program.function(target).unwrap();

        assert_eq!(
            String::from_utf8(render_function(&program, function).unwrap()).unwrap(),
            concat!(
                "scoreboard players set #f0v0 mdl.reg -2147483648\n",
                "scoreboard players set #f0v1 mdl.reg -1\n",
                "scoreboard players set #f0v2 mdl.reg 1\n",
                "scoreboard players operation #f0v3 mdl.reg = #f0v0 mdl.reg\n",
                "scoreboard players operation #f0v3 mdl.reg += #f0v1 mdl.reg\n",
                "scoreboard players operation #f0v4 mdl.reg = #f0v0 mdl.reg\n",
                "scoreboard players operation #f0v4 mdl.reg += #f0v1 mdl.reg\n",
                "scoreboard players set #f0v5 mdl.reg 0\n",
                "execute if score #f0v0 mdl.reg matches 0.. if score #f0v1 mdl.reg matches 0.. if score #f0v4 mdl.reg matches ..-1 run scoreboard players set #f0v5 mdl.reg 1\n",
                "execute if score #f0v0 mdl.reg matches ..-1 if score #f0v1 mdl.reg matches ..-1 if score #f0v4 mdl.reg matches 0.. run scoreboard players set #f0v5 mdl.reg 1\n",
                "scoreboard players set #f0v6 mdl.reg 0\n",
                "execute unless score #f0v0 mdl.reg = #f0v1 mdl.reg run scoreboard players set #f0v6 mdl.reg 1\n",
                "scoreboard players set #f0v7 mdl.reg 1\n",
                "scoreboard players operation #f0v7 mdl.reg -= #f0v2 mdl.reg\n"
            )
        );
        assert!(
            function
                .body()
                .commands()
                .all(|(_, command)| command.origin() == instruction_origin)
        );
    }

    #[test]
    fn signed_overflow_rule_matches_rust_at_boundaries() {
        let values = [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX];
        for left in values {
            for right in values {
                let (sum, expected) = left.overflowing_add(right);
                let observed =
                    (left >= 0 && right >= 0 && sum < 0) || (left < 0 && right < 0 && sum >= 0);
                assert_eq!(observed, expected, "{left} + {right}");
            }
        }
    }

    #[test]
    fn all_signed_predicates_have_explicit_target_conditions() {
        use I32Predicate::{Eq, Ne, SignedGe, SignedGt, SignedLe, SignedLt};
        assert_eq!(
            [Eq, Ne, SignedLt, SignedLe, SignedGt, SignedGe].map(super::comparison_condition),
            [
                (true, ScoreComparison::Equal),
                (false, ScoreComparison::Equal),
                (true, ScoreComparison::LessThan),
                (true, ScoreComparison::LessOrEqual),
                (true, ScoreComparison::GreaterThan),
                (true, ScoreComparison::GreaterOrEqual),
            ]
        );
    }

    fn options() -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
    }
}
