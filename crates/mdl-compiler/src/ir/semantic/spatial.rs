//! Exact compiler-known spatial attributes.

use std::error::Error;
use std::fmt;

/// Maximum accepted bytes in one source decimal literal.
pub const MAX_FINITE_DECIMAL_BYTES: usize = 128;
/// Maximum accepted decimal digits, excluding sign and decimal point.
pub const MAX_FINITE_DECIMAL_DIGITS: usize = 96;

/// An exact, finite, canonically printed base-ten literal.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FiniteDecimal(Box<str>);

impl FiniteDecimal {
    /// Parses the Stage 7.5 decimal grammar without involving host floating point.
    ///
    /// # Errors
    ///
    /// Returns a precise bounded-grammar error for invalid input.
    pub fn parse(input: &str) -> Result<Self, FiniteDecimalError> {
        if input.is_empty() {
            return Err(FiniteDecimalError::Empty);
        }
        if input.len() > MAX_FINITE_DECIMAL_BYTES {
            return Err(FiniteDecimalError::TooLong);
        }
        let unsigned = input
            .strip_prefix('-')
            .or_else(|| input.strip_prefix('+'))
            .unwrap_or(input);
        if unsigned.is_empty() {
            return Err(FiniteDecimalError::MissingDigits);
        }
        let mut pieces = unsigned.split('.');
        let integer = pieces.next().unwrap_or_default();
        let fraction = pieces.next();
        if pieces.next().is_some() {
            return Err(FiniteDecimalError::MultipleDecimalPoints);
        }
        if integer.is_empty() || !integer.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(FiniteDecimalError::InvalidCharacter);
        }
        if fraction
            .is_some_and(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(FiniteDecimalError::InvalidCharacter);
        }
        let digits = integer.len() + fraction.map_or(0, str::len);
        if digits > MAX_FINITE_DECIMAL_DIGITS {
            return Err(FiniteDecimalError::TooManyDigits);
        }

        let normalized_integer = integer.trim_start_matches('0');
        let normalized_integer = if normalized_integer.is_empty() {
            "0"
        } else {
            normalized_integer
        };
        let normalized_fraction = fraction.map_or("", |part| part.trim_end_matches('0'));
        let is_zero = normalized_integer == "0" && normalized_fraction.is_empty();
        let negative = input.starts_with('-') && !is_zero;
        let mut canonical = String::with_capacity(input.len());
        if negative {
            canonical.push('-');
        }
        canonical.push_str(normalized_integer);
        if !normalized_fraction.is_empty() {
            canonical.push('.');
            canonical.push_str(normalized_fraction);
        }
        Ok(Self(canonical.into_boxed_str()))
    }

    /// Canonical exact decimal spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Canonical zero.
    #[must_use]
    pub fn zero() -> Self {
        Self("0".into())
    }

    /// Whether this exact value is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.as_str() == "0"
    }
}

impl fmt::Display for FiniteDecimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Why an exact decimal was rejected.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FiniteDecimalError {
    Empty,
    TooLong,
    MissingDigits,
    MultipleDecimalPoints,
    InvalidCharacter,
    TooManyDigits,
}

impl fmt::Display for FiniteDecimalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "decimal literal cannot be empty",
            Self::TooLong => "decimal literal exceeds the byte limit",
            Self::MissingDigits => "decimal literal requires digits",
            Self::MultipleDecimalPoints => "decimal literal contains multiple decimal points",
            Self::InvalidCharacter => "decimal literal has an invalid character or fraction",
            Self::TooManyDigits => "decimal literal exceeds the digit limit",
        })
    }
}

impl Error for FiniteDecimalError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum WorldAxis {
    Absolute(FiniteDecimal),
    Relative(FiniteDecimal),
}

impl WorldAxis {
    #[must_use]
    pub fn reads_position(&self) -> bool {
        matches!(self, Self::Relative(_))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct WorldPosition {
    pub x: WorldAxis,
    pub y: WorldAxis,
    pub z: WorldAxis,
}

impl WorldPosition {
    #[must_use]
    pub fn reads_position(&self) -> bool {
        self.x.reads_position() || self.y.reads_position() || self.z.reads_position()
    }
}

/// An absolute integer block coordinate triple.
///
/// Deliberately not `WorldPosition`: block positions are always integer, with
/// no `~`-relative or fractional form in this slice (see
/// `notes/compiler/block-entity-nbt-paths.md` §1.3-1.4) — `WorldAxis`/
/// `FiniteDecimal` model a different, decimal-valued argument shape entirely.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BlockPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LocalPosition {
    pub left: FiniteDecimal,
    pub up: FiniteDecimal,
    pub forward: FiniteDecimal,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum PositionSpec {
    World(WorldPosition),
    Local(LocalPosition),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum RotationAxis {
    Absolute(FiniteDecimal),
    Relative(FiniteDecimal),
}

impl RotationAxis {
    #[must_use]
    pub fn reads_rotation(&self) -> bool {
        matches!(self, Self::Relative(_))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RotationSpec {
    pub yaw: RotationAxis,
    pub pitch: RotationAxis,
}

impl RotationSpec {
    #[must_use]
    pub fn reads_rotation(&self) -> bool {
        self.yaw.reads_rotation() || self.pitch.reads_rotation()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RelativeWorldOffset {
    pub x: FiniteDecimal,
    pub y: FiniteDecimal,
    pub z: FiniteDecimal,
}

/// Closed Stage 7.5 dimension constants.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DimensionKey {
    Overworld,
    TheNether,
    TheEnd,
}

impl DimensionKey {
    #[must_use]
    pub const fn resource(self) -> &'static str {
        match self {
            Self::Overworld => "minecraft:overworld",
            Self::TheNether => "minecraft:the_nether",
            Self::TheEnd => "minecraft:the_end",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntityAnchor {
    Feet,
    Eyes,
}

/// A nonempty canonical subset of the three coordinate axes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Axes(u8);

impl Axes {
    pub const X: u8 = 1;
    pub const Y: u8 = 2;
    pub const Z: u8 = 4;

    #[must_use]
    pub const fn new(bits: u8) -> Option<Self> {
        if bits != 0 && bits & !(Self::X | Self::Y | Self::Z) == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self.0 {
            1 => "x",
            2 => "y",
            3 => "xy",
            4 => "z",
            5 => "xz",
            6 => "yz",
            7 => "xyz",
            _ => unreachable!("Axes invariant"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_equality_is_numeric_and_exact() {
        for spelling in ["0", "-0", "+000.000", "000"] {
            assert_eq!(
                FiniteDecimal::parse(spelling).unwrap(),
                FiniteDecimal::zero()
            );
        }
        assert_eq!(FiniteDecimal::parse("+001.2300").unwrap().as_str(), "1.23");
        assert_eq!(FiniteDecimal::parse("-001.2300").unwrap().as_str(), "-1.23");
    }

    #[test]
    fn decimal_validation_is_bounded_and_linear() {
        let too_many = "1".repeat(MAX_FINITE_DECIMAL_DIGITS + 1);
        assert_eq!(
            FiniteDecimal::parse(&too_many),
            Err(FiniteDecimalError::TooManyDigits)
        );
        for invalid in ["", ".1", "1.", "1e2", "NaN", "--1", "1.2.3"] {
            assert!(
                FiniteDecimal::parse(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn axes_are_nonempty_and_canonical() {
        assert!(Axes::new(0).is_none());
        assert!(Axes::new(8).is_none());
        assert_eq!(Axes::new(Axes::Z | Axes::X).unwrap().as_str(), "xz");
    }
}
