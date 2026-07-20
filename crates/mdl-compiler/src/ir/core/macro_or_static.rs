use super::ValueId;

/// A value that is either known at compile time or must be resolved across a
/// `.mcfunction` boundary via Minecraft macro `$(variable)` substitution.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MacroOrStatic<T> {
    /// Compile-time constant. The existing static lowering applies.
    Static(T),
    /// Runtime value identified by a Core IR SSA `ValueId`. The lowering
    /// creates a macro helper `.mcfunction` where this value is substituted
    /// into command syntax via `$(variable)` expansion.
    Macro(ValueId),
}
