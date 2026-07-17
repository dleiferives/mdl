//! Java-validated structured spatial command atoms.

use std::error::Error;
use std::fmt;

use crate::ir::semantic::FiniteDecimal;

/// Exact decimal accepted by the selected Java command parser.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JavaDecimal(Box<str>);

impl JavaDecimal {
    /// Validates exact decimal grammar, canonicalizes it, and proves Java's double parser is finite.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid exact-decimal syntax or a non-finite Java double.
    pub fn new(value: &str) -> Result<Self, JavaDecimalError> {
        let exact = FiniteDecimal::parse(value).map_err(JavaDecimalError::InvalidDecimal)?;
        let parsed = exact
            .as_str()
            .parse::<f64>()
            .map_err(|_| JavaDecimalError::OutsideJavaDouble)?;
        if !parsed.is_finite() {
            return Err(JavaDecimalError::OutsideJavaDouble);
        }
        Ok(Self(exact.as_str().into()))
    }

    /// Returns the canonical command atom.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JavaDecimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Failed Java decimal validation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JavaDecimalError {
    /// The exact source-independent decimal grammar was invalid.
    InvalidDecimal(crate::ir::semantic::FiniteDecimalError),
    /// The value cannot be represented by Java's finite double parser.
    OutsideJavaDouble,
}

impl fmt::Display for JavaDecimalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDecimal(error) => write!(formatter, "{error}"),
            Self::OutsideJavaDouble => {
                formatter.write_str("decimal is outside Java finite-double range")
            }
        }
    }
}

impl Error for JavaDecimalError {}

/// One absolute or `~`-relative target coordinate.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TargetWorldAxis {
    /// Absolute coordinate.
    Absolute(JavaDecimal),
    /// Relative coordinate offset.
    Relative(JavaDecimal),
}

/// A structured Java `vec3` world-coordinate triple.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TargetWorldPosition {
    /// X coordinate.
    pub x: TargetWorldAxis,
    /// Y coordinate.
    pub y: TargetWorldAxis,
    /// Z coordinate.
    pub z: TargetWorldAxis,
}

/// A structured all-local Java `vec3` triple.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TargetLocalPosition {
    /// Left offset.
    pub left: JavaDecimal,
    /// Up offset.
    pub up: JavaDecimal,
    /// Forward offset.
    pub forward: JavaDecimal,
}

/// One Java `minecraft:vec3` argument.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TargetPosition {
    /// Absolute/relative world-coordinate family.
    World(TargetWorldPosition),
    /// All-local coordinate family.
    Local(TargetLocalPosition),
}

/// One absolute or `~`-relative rotation coordinate.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TargetRotationAxis {
    /// Absolute angle.
    Absolute(JavaDecimal),
    /// Relative angle offset.
    Relative(JavaDecimal),
}

/// Java `minecraft:rotation` yaw and pitch.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TargetRotation {
    /// Yaw component.
    pub yaw: TargetRotationAxis,
    /// Pitch component.
    pub pitch: TargetRotationAxis,
}

/// Java entity anchor.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TargetAnchor {
    /// Entity feet.
    Feet,
    /// Entity eyes.
    Eyes,
}

/// Nonempty Java swizzle rendered canonically.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TargetAxes(u8);

impl TargetAxes {
    /// Constructs a nonempty subset of x/y/z bits.
    #[must_use]
    pub const fn new(bits: u8) -> Option<Self> {
        if bits != 0 && bits & !7 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// Canonical `xyz`-ordered spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self.0 {
            1 => "x",
            2 => "y",
            3 => "xy",
            4 => "z",
            5 => "xz",
            6 => "yz",
            7 => "xyz",
            _ => unreachable!(),
        }
    }
}

/// Selected structured `teleport @s <position>` form.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TeleportCommand {
    destination: TargetPosition,
}

impl TeleportCommand {
    /// Constructs a current-executor teleport.
    #[must_use]
    pub const fn current_executor(destination: TargetPosition) -> Self {
        Self { destination }
    }

    /// Returns the structured destination.
    #[must_use]
    pub const fn destination(&self) -> &TargetPosition {
        &self.destination
    }
}

impl fmt::Display for TargetWorldAxis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute(value) => value.fmt(formatter),
            Self::Relative(value) if value.as_str() == "0" => formatter.write_str("~"),
            Self::Relative(value) => write!(formatter, "~{value}"),
        }
    }
}

impl fmt::Display for TargetRotationAxis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute(value) => value.fmt(formatter),
            Self::Relative(value) if value.as_str() == "0" => formatter.write_str("~"),
            Self::Relative(value) => write!(formatter, "~{value}"),
        }
    }
}

impl fmt::Display for TargetPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::World(value) => write!(formatter, "{} {} {}", value.x, value.y, value.z),
            Self::Local(value) => {
                fn local(formatter: &mut fmt::Formatter<'_>, value: &JavaDecimal) -> fmt::Result {
                    if value.as_str() == "0" {
                        formatter.write_str("^")
                    } else {
                        write!(formatter, "^{value}")
                    }
                }
                local(formatter, &value.left)?;
                formatter.write_str(" ")?;
                local(formatter, &value.up)?;
                formatter.write_str(" ")?;
                local(formatter, &value.forward)
            }
        }
    }
}

impl fmt::Display for TargetRotation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.yaw, self.pitch)
    }
}
