use super::ValueId;

/// A value that is either known at compile time or must be resolved across a
/// `.mcfunction` boundary via Minecraft macro `$(variable)` substitution.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Operand<T> {
    /// Compile-time constant. The existing static lowering applies.
    Const(T),
    /// Runtime value identified by a Core IR SSA `ValueId`. The lowering
    /// creates a macro helper `.mcfunction` where this value is substituted
    /// into command syntax via `$(variable)` expansion.
    Runtime(ValueId),
}

impl<T> Operand<T> {
    /// Returns `true` if this is a compile-time constant.
    #[must_use]
    pub const fn is_const(&self) -> bool {
        matches!(self, Self::Const(_))
    }

    /// Returns the constant value, or `None` if runtime.
    #[must_use]
    pub const fn as_const(&self) -> Option<&T> {
        match self {
            Self::Const(value) => Some(value),
            Self::Runtime(_) => None,
        }
    }

    /// Returns the runtime `ValueId`, or `None` if constant.
    #[must_use]
    pub const fn as_runtime(&self) -> Option<ValueId> {
        match self {
            Self::Runtime(value) => Some(*value),
            Self::Const(_) => None,
        }
    }

    /// Transforms the constant value with `f`, leaving a runtime value unchanged.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Operand<U> {
        match self {
            Self::Const(value) => Operand::Const(f(value)),
            Self::Runtime(value) => Operand::Runtime(value),
        }
    }
}
