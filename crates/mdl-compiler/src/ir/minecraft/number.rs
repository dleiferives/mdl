use std::error::Error;
use std::fmt;

/// The target floating-point width being validated.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FloatWidth {
    /// IEEE-754 binary32.
    F32,
    /// IEEE-754 binary64.
    F64,
}

impl fmt::Display for FloatWidth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::F32 => "f32",
            Self::F64 => "f64",
        })
    }
}

/// The class of non-finite value rejected by target syntax.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NonFiniteKind {
    /// Not a number.
    NaN,
    /// Positive infinity.
    PositiveInfinity,
    /// Negative infinity.
    NegativeInfinity,
}

/// A non-finite floating-point value rejected by a finite wrapper.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NonFiniteFloat {
    width: FloatWidth,
    kind: NonFiniteKind,
}

impl NonFiniteFloat {
    const fn new(width: FloatWidth, kind: NonFiniteKind) -> Self {
        Self { width, kind }
    }

    /// Returns the rejected floating-point width.
    #[must_use]
    pub const fn width(self) -> FloatWidth {
        self.width
    }

    /// Returns the rejected non-finite class.
    #[must_use]
    pub const fn kind(self) -> NonFiniteKind {
        self.kind
    }
}

impl fmt::Display for NonFiniteFloat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} value is not finite: ", self.width)?;
        formatter.write_str(match self.kind {
            NonFiniteKind::NaN => "NaN",
            NonFiniteKind::PositiveInfinity => "+infinity",
            NonFiniteKind::NegativeInfinity => "-infinity",
        })
    }
}

impl Error for NonFiniteFloat {}

/// A finite `f32` suitable for command and SNBT rendering.
///
/// Signed zero is preserved. `PartialEq` retains Rust floating-point semantics, so
/// positive and negative zero compare equal even though their stored bits differ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FiniteF32(f32);

impl FiniteF32 {
    /// Validates one finite `f32`.
    ///
    /// # Errors
    ///
    /// Rejects NaN and positive or negative infinity.
    pub fn new(value: f32) -> Result<Self, NonFiniteFloat> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(NonFiniteFloat::new(
                FloatWidth::F32,
                classify_non_finite_f32(value),
            ))
        }
    }

    /// Returns the finite value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Returns the exact IEEE-754 representation, including signed zero.
    #[must_use]
    pub const fn to_bits(self) -> u32 {
        self.0.to_bits()
    }
}

impl TryFrom<f32> for FiniteF32 {
    type Error = NonFiniteFloat;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<FiniteF32> for f32 {
    fn from(value: FiniteF32) -> Self {
        value.get()
    }
}

impl fmt::Display for FiniteF32 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A finite `f64` suitable for command and SNBT rendering.
///
/// Signed zero is preserved. `PartialEq` retains Rust floating-point semantics, so
/// positive and negative zero compare equal even though their stored bits differ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FiniteF64(f64);

impl FiniteF64 {
    /// Validates one finite `f64`.
    ///
    /// # Errors
    ///
    /// Rejects NaN and positive or negative infinity.
    pub fn new(value: f64) -> Result<Self, NonFiniteFloat> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(NonFiniteFloat::new(
                FloatWidth::F64,
                classify_non_finite_f64(value),
            ))
        }
    }

    /// Returns the finite value.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// Returns the exact IEEE-754 representation, including signed zero.
    #[must_use]
    pub const fn to_bits(self) -> u64 {
        self.0.to_bits()
    }
}

impl TryFrom<f64> for FiniteF64 {
    type Error = NonFiniteFloat;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<FiniteF64> for f64 {
    fn from(value: FiniteF64) -> Self {
        value.get()
    }
}

impl fmt::Display for FiniteF64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

fn classify_non_finite_f32(value: f32) -> NonFiniteKind {
    if value.is_nan() {
        NonFiniteKind::NaN
    } else if value.is_sign_positive() {
        NonFiniteKind::PositiveInfinity
    } else {
        NonFiniteKind::NegativeInfinity
    }
}

fn classify_non_finite_f64(value: f64) -> NonFiniteKind {
    if value.is_nan() {
        NonFiniteKind::NaN
    } else if value.is_sign_positive() {
        NonFiniteKind::PositiveInfinity
    } else {
        NonFiniteKind::NegativeInfinity
    }
}

#[cfg(test)]
mod tests {
    use super::{FiniteF32, FiniteF64, FloatWidth, NonFiniteKind};

    #[test]
    fn finite_wrappers_reject_every_non_finite_class() {
        let f32_cases = [
            (f32::NAN, NonFiniteKind::NaN),
            (f32::INFINITY, NonFiniteKind::PositiveInfinity),
            (f32::NEG_INFINITY, NonFiniteKind::NegativeInfinity),
        ];
        for (value, expected) in f32_cases {
            let error = FiniteF32::new(value).unwrap_err();
            assert_eq!(error.width(), FloatWidth::F32);
            assert_eq!(error.kind(), expected);
        }

        let f64_cases = [
            (f64::NAN, NonFiniteKind::NaN),
            (f64::INFINITY, NonFiniteKind::PositiveInfinity),
            (f64::NEG_INFINITY, NonFiniteKind::NegativeInfinity),
        ];
        for (value, expected) in f64_cases {
            let error = FiniteF64::new(value).unwrap_err();
            assert_eq!(error.width(), FloatWidth::F64);
            assert_eq!(error.kind(), expected);
        }
    }

    #[test]
    fn signed_zero_bits_and_target_text_are_preserved() {
        let positive32 = FiniteF32::new(0.0).unwrap();
        let negative32 = FiniteF32::new(-0.0).unwrap();
        assert_eq!(positive32, negative32);
        assert_ne!(positive32.to_bits(), negative32.to_bits());
        assert_eq!(positive32.to_string(), "0");
        assert_eq!(negative32.to_string(), "-0");

        let positive64 = FiniteF64::new(0.0).unwrap();
        let negative64 = FiniteF64::new(-0.0).unwrap();
        assert_eq!(positive64, negative64);
        assert_ne!(positive64.to_bits(), negative64.to_bits());
        assert_eq!(positive64.to_string(), "0");
        assert_eq!(negative64.to_string(), "-0");
    }

    #[test]
    fn finite_values_use_rusts_shortest_round_trip_format() {
        assert_eq!(FiniteF32::new(1.5).unwrap().to_string(), "1.5");
        assert_eq!(FiniteF64::new(-12.25).unwrap().to_string(), "-12.25");

        let f32_value = FiniteF32::new(f32::MIN_POSITIVE).unwrap();
        let f32_text = f32_value.to_string();
        assert_eq!(
            f32_text.parse::<f32>().unwrap().to_bits(),
            f32_value.to_bits()
        );

        let f64_value = FiniteF64::new(f64::MAX).unwrap();
        let f64_text = f64_value.to_string();
        assert_eq!(
            f64_text.parse::<f64>().unwrap().to_bits(),
            f64_value.to_bits()
        );
    }
}
