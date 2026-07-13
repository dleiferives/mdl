use crate::ir::core::{CoreType, I32Predicate, TypedCoreConstant};

/// One cell in SCCP's typed flat scalar lattice.
///
/// The immutable `CoreType` belongs to the SSA value rather than this enum.  Keeping
/// the two constant domains distinct still makes an accidentally cross-typed join a
/// checked solver invariant instead of silently widening it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LatticeValue {
    Unknown,
    BoolConstant(bool),
    I32Constant(i32),
    Overdefined,
}

impl LatticeValue {
    pub(super) const fn constant(self) -> Option<TypedCoreConstant> {
        match self {
            Self::BoolConstant(value) => Some(TypedCoreConstant::Bool(value)),
            Self::I32Constant(value) => Some(TypedCoreConstant::I32(value)),
            Self::Unknown | Self::Overdefined => None,
        }
    }

    const fn constant_type(self) -> Option<CoreType> {
        match self {
            Self::BoolConstant(_) => Some(CoreType::Bool),
            Self::I32Constant(_) => Some(CoreType::I32),
            Self::Unknown | Self::Overdefined => None,
        }
    }

    /// Monotonically joins `incoming` into this cell.
    ///
    /// Returns whether the cell changed.  Both the old and incoming fact are checked
    /// against the SSA value's immutable type before applying the flat-lattice join.
    pub(super) fn join(&mut self, ty: CoreType, incoming: Self) -> Result<bool, LatticeTypeError> {
        Self::check_type(ty, *self)?;
        Self::check_type(ty, incoming)?;

        let joined = match (*self, incoming) {
            (Self::Overdefined, _) | (_, Self::Overdefined) => Self::Overdefined,
            (Self::Unknown, other) => other,
            (current, Self::Unknown) => current,
            (current, other) if current == other => current,
            (Self::BoolConstant(_), Self::BoolConstant(_))
            | (Self::I32Constant(_), Self::I32Constant(_)) => Self::Overdefined,
            // Cross-typed pairs were rejected above.
            (Self::BoolConstant(_), Self::I32Constant(_))
            | (Self::I32Constant(_), Self::BoolConstant(_)) => unreachable!(),
        };
        let changed = joined != *self;
        *self = joined;
        Ok(changed)
    }

    fn check_type(ty: CoreType, value: Self) -> Result<(), LatticeTypeError> {
        if let Some(actual) = value.constant_type() {
            if actual != ty {
                return Err(LatticeTypeError {
                    expected: ty,
                    actual,
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::opt::core) struct LatticeTypeError {
    pub(super) expected: CoreType,
    pub(super) actual: CoreType,
}

pub(super) fn bool_not(operand: LatticeValue) -> Result<LatticeValue, LatticeTypeError> {
    match operand {
        LatticeValue::Unknown => Ok(LatticeValue::Unknown),
        LatticeValue::BoolConstant(value) => Ok(LatticeValue::BoolConstant(!value)),
        LatticeValue::Overdefined => Ok(LatticeValue::Overdefined),
        LatticeValue::I32Constant(_) => Err(LatticeTypeError {
            expected: CoreType::Bool,
            actual: CoreType::I32,
        }),
    }
}

pub(super) fn i32_add_wrapping(
    lhs: LatticeValue,
    rhs: LatticeValue,
) -> Result<LatticeValue, LatticeTypeError> {
    check_i32_operand(lhs)?;
    check_i32_operand(rhs)?;
    Ok(match (lhs, rhs) {
        (LatticeValue::I32Constant(lhs), LatticeValue::I32Constant(rhs)) => {
            LatticeValue::I32Constant(lhs.wrapping_add(rhs))
        }
        (LatticeValue::Overdefined, _) | (_, LatticeValue::Overdefined) => {
            LatticeValue::Overdefined
        }
        _ => LatticeValue::Unknown,
    })
}

pub(super) fn i32_add_overflowing(
    lhs: LatticeValue,
    rhs: LatticeValue,
) -> Result<[LatticeValue; 2], LatticeTypeError> {
    check_i32_operand(lhs)?;
    check_i32_operand(rhs)?;

    if let (LatticeValue::I32Constant(lhs), LatticeValue::I32Constant(rhs)) = (lhs, rhs) {
        let (sum, overflowed) = lhs.overflowing_add(rhs);
        return Ok([
            LatticeValue::I32Constant(sum),
            LatticeValue::BoolConstant(overflowed),
        ]);
    }

    if lhs == LatticeValue::I32Constant(0) {
        return Ok([rhs, LatticeValue::BoolConstant(false)]);
    }
    if rhs == LatticeValue::I32Constant(0) {
        return Ok([lhs, LatticeValue::BoolConstant(false)]);
    }

    if matches!(lhs, LatticeValue::Overdefined) || matches!(rhs, LatticeValue::Overdefined) {
        return Ok([LatticeValue::Overdefined, LatticeValue::Overdefined]);
    }
    Ok([LatticeValue::Unknown, LatticeValue::Unknown])
}

pub(super) fn i32_compare(
    predicate: I32Predicate,
    lhs: LatticeValue,
    rhs: LatticeValue,
    operands_are_identical: bool,
) -> Result<LatticeValue, LatticeTypeError> {
    check_i32_operand(lhs)?;
    check_i32_operand(rhs)?;

    if operands_are_identical {
        return Ok(LatticeValue::BoolConstant(match predicate {
            I32Predicate::Eq | I32Predicate::SignedLe | I32Predicate::SignedGe => true,
            I32Predicate::Ne | I32Predicate::SignedLt | I32Predicate::SignedGt => false,
        }));
    }

    Ok(match (lhs, rhs) {
        (LatticeValue::I32Constant(lhs), LatticeValue::I32Constant(rhs)) => {
            LatticeValue::BoolConstant(match predicate {
                I32Predicate::Eq => lhs == rhs,
                I32Predicate::Ne => lhs != rhs,
                I32Predicate::SignedLt => lhs < rhs,
                I32Predicate::SignedLe => lhs <= rhs,
                I32Predicate::SignedGt => lhs > rhs,
                I32Predicate::SignedGe => lhs >= rhs,
            })
        }
        (LatticeValue::Overdefined, _) | (_, LatticeValue::Overdefined) => {
            LatticeValue::Overdefined
        }
        _ => LatticeValue::Unknown,
    })
}

fn check_i32_operand(value: LatticeValue) -> Result<(), LatticeTypeError> {
    if matches!(value, LatticeValue::BoolConstant(_)) {
        Err(LatticeTypeError {
            expected: CoreType::I32,
            actual: CoreType::Bool,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LatticeTypeError, LatticeValue, bool_not, i32_add_overflowing, i32_add_wrapping,
        i32_compare,
    };
    use crate::ir::core::{CoreType, I32Predicate, TypedCoreConstant};

    #[test]
    fn flat_join_table_is_monotone_for_each_type() {
        let bool_values = [
            LatticeValue::Unknown,
            LatticeValue::BoolConstant(false),
            LatticeValue::BoolConstant(true),
            LatticeValue::Overdefined,
        ];
        let i32_values = [
            LatticeValue::Unknown,
            LatticeValue::I32Constant(-1),
            LatticeValue::I32Constant(4),
            LatticeValue::Overdefined,
        ];

        for (ty, values) in [
            (CoreType::Bool, &bool_values[..]),
            (CoreType::I32, &i32_values[..]),
        ] {
            for left in values {
                for right in values {
                    let mut forward = *left;
                    forward.join(ty, *right).unwrap();
                    let mut reverse = *right;
                    reverse.join(ty, *left).unwrap();
                    assert_eq!(forward, reverse, "join must be commutative");

                    let once = forward;
                    assert!(!forward.join(ty, *right).unwrap());
                    assert_eq!(forward, once, "join must be idempotent");

                    for third in values {
                        let mut left_associated = *left;
                        left_associated.join(ty, *right).unwrap();
                        left_associated.join(ty, *third).unwrap();

                        let mut right_pair = *right;
                        right_pair.join(ty, *third).unwrap();
                        let mut right_associated = *left;
                        right_associated.join(ty, right_pair).unwrap();
                        assert_eq!(
                            left_associated, right_associated,
                            "join must be associative"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn joins_reject_cross_typed_constants() {
        let mut value = LatticeValue::Unknown;
        assert_eq!(
            value
                .join(CoreType::Bool, LatticeValue::I32Constant(1))
                .unwrap_err(),
            LatticeTypeError {
                expected: CoreType::Bool,
                actual: CoreType::I32,
            }
        );
        assert_eq!(
            LatticeValue::BoolConstant(true).constant(),
            Some(TypedCoreConstant::Bool(true))
        );
    }

    #[test]
    fn scalar_transfers_cover_partial_and_exact_facts() {
        assert_eq!(
            bool_not(LatticeValue::BoolConstant(true)).unwrap(),
            LatticeValue::BoolConstant(false)
        );
        assert_eq!(
            i32_add_wrapping(
                LatticeValue::I32Constant(i32::MAX),
                LatticeValue::I32Constant(1),
            )
            .unwrap(),
            LatticeValue::I32Constant(i32::MIN)
        );
        assert_eq!(
            i32_add_overflowing(LatticeValue::Unknown, LatticeValue::I32Constant(0),).unwrap(),
            [LatticeValue::Unknown, LatticeValue::BoolConstant(false)]
        );
        assert_eq!(
            i32_add_overflowing(LatticeValue::Overdefined, LatticeValue::I32Constant(0),).unwrap(),
            [LatticeValue::Overdefined, LatticeValue::BoolConstant(false),]
        );
    }

    #[test]
    fn identical_compare_operands_fold_without_runtime_facts() {
        for (predicate, expected) in [
            (I32Predicate::Eq, true),
            (I32Predicate::Ne, false),
            (I32Predicate::SignedLt, false),
            (I32Predicate::SignedLe, true),
            (I32Predicate::SignedGt, false),
            (I32Predicate::SignedGe, true),
        ] {
            assert_eq!(
                i32_compare(
                    predicate,
                    LatticeValue::Unknown,
                    LatticeValue::Unknown,
                    true,
                )
                .unwrap(),
                LatticeValue::BoolConstant(expected)
            );
            assert_eq!(
                i32_compare(
                    predicate,
                    LatticeValue::Overdefined,
                    LatticeValue::Overdefined,
                    true,
                )
                .unwrap(),
                LatticeValue::BoolConstant(expected)
            );
        }
    }

    #[test]
    fn every_i32_predicate_obeys_signed_boundary_semantics() {
        let cases = [
            (I32Predicate::Eq, false),
            (I32Predicate::Ne, true),
            (I32Predicate::SignedLt, true),
            (I32Predicate::SignedLe, true),
            (I32Predicate::SignedGt, false),
            (I32Predicate::SignedGe, false),
        ];
        for (predicate, expected) in cases {
            assert_eq!(
                i32_compare(
                    predicate,
                    LatticeValue::I32Constant(i32::MIN),
                    LatticeValue::I32Constant(i32::MAX),
                    false,
                )
                .unwrap(),
                LatticeValue::BoolConstant(expected),
            );
        }

        assert_eq!(
            i32_add_overflowing(
                LatticeValue::I32Constant(i32::MAX),
                LatticeValue::I32Constant(1),
            )
            .unwrap(),
            [
                LatticeValue::I32Constant(i32::MIN),
                LatticeValue::BoolConstant(true),
            ]
        );
        assert_eq!(
            i32_add_overflowing(
                LatticeValue::I32Constant(i32::MIN),
                LatticeValue::I32Constant(-1),
            )
            .unwrap(),
            [
                LatticeValue::I32Constant(i32::MAX),
                LatticeValue::BoolConstant(true),
            ]
        );
    }
}
