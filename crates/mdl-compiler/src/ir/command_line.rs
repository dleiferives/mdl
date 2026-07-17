//! Shared validation for one physical Minecraft command line.
//!
//! Shape is target-independent. Encoding and logical-length limits remain owned by
//! the selected target and are deliberately not checked here.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CommandLineShapeError {
    Empty,
    PhysicalNewline,
    BoundaryWhitespace,
    ReservedPrefix(char),
    TerminalContinuation,
}

impl fmt::Display for CommandLineShapeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("command cannot be empty"),
            Self::PhysicalNewline => formatter.write_str("command cannot contain CR or LF"),
            Self::BoundaryWhitespace => {
                formatter.write_str("command cannot begin or end with whitespace")
            }
            Self::ReservedPrefix(prefix) => {
                write!(formatter, "command cannot start with {prefix:?}")
            }
            Self::TerminalContinuation => {
                formatter.write_str("command cannot end with a continuation backslash")
            }
        }
    }
}

pub(crate) fn validate_command_line_shape(line: &str) -> Result<(), CommandLineShapeError> {
    if line.is_empty() {
        return Err(CommandLineShapeError::Empty);
    }
    if line.contains(['\r', '\n']) {
        return Err(CommandLineShapeError::PhysicalNewline);
    }
    if line.starts_with(char::is_whitespace) || line.ends_with(char::is_whitespace) {
        return Err(CommandLineShapeError::BoundaryWhitespace);
    }
    if let Some(prefix @ ('/' | '#' | '$')) = line.chars().next() {
        return Err(CommandLineShapeError::ReservedPrefix(prefix));
    }
    if line.ends_with('\\') {
        return Err(CommandLineShapeError::TerminalContinuation);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CommandLineShapeError, validate_command_line_shape};

    #[test]
    fn validates_only_target_independent_physical_shape() {
        assert!(validate_command_line_shape("say hi").is_ok());
        for (line, expected) in [
            ("", CommandLineShapeError::Empty),
            ("say\nhi", CommandLineShapeError::PhysicalNewline),
            (" say hi", CommandLineShapeError::BoundaryWhitespace),
            ("/say hi", CommandLineShapeError::ReservedPrefix('/')),
            ("say hi\\", CommandLineShapeError::TerminalContinuation),
        ] {
            assert_eq!(validate_command_line_shape(line), Err(expected));
        }
    }
}
