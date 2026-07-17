use std::error::Error;
use std::fmt;

use crate::ir::semantic::MessageLiteral;
use crate::target::JavaEditionTarget;

/// Java 26.2's native `minecraft:message` argument limit for `say`.
///
/// Minecraft measures this bound with Java's UTF-16 string length, not UTF-8
/// bytes or Unicode scalar values.
pub const MAX_SAY_MESSAGE_UTF16_UNITS: usize = 256;

/// Why a message cannot be represented by the structured `say` command.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SayMessageError {
    /// The message contains no text.
    Empty,
    /// Leading or trailing whitespace would not have a proven round trip through
    /// the native message argument parser.
    BoundaryWhitespace,
    /// A control character is not accepted by the initial literal-message subset.
    ControlCharacter {
        /// UTF-8 byte offset of the rejected character.
        index: usize,
        /// Rejected Unicode scalar value.
        character: char,
    },
    /// `@` may begin native selector interpolation, which has different effects
    /// from the initial literal-message subset.
    SelectorInterpolation {
        /// UTF-8 byte offset of the rejected `@`.
        index: usize,
    },
    /// A final backslash would continue the physical `.mcfunction` line.
    TerminalContinuation,
    /// The message exceeds the selected target's native argument bound.
    TooLong {
        /// Observed Java UTF-16 code-unit count.
        units: usize,
        /// Target maximum.
        max: usize,
    },
}

impl fmt::Display for SayMessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("say message cannot be empty"),
            Self::BoundaryWhitespace => {
                formatter.write_str("say message cannot begin or end with whitespace")
            }
            Self::ControlCharacter { index, character } => write!(
                formatter,
                "say message contains control character {character:?} at byte {index}"
            ),
            Self::SelectorInterpolation { index } => write!(
                formatter,
                "say message contains unsupported selector interpolation at byte {index}"
            ),
            Self::TerminalContinuation => {
                formatter.write_str("say message cannot end with a continuation backslash")
            }
            Self::TooLong { units, max } => write!(
                formatter,
                "say message has {units} UTF-16 units; maximum is {max}"
            ),
        }
    }
}

impl Error for SayMessageError {}

/// One validated literal message for Minecraft's native `say` command.
///
/// This deliberately models a smaller language than the full
/// `minecraft:message` argument: selector interpolation and physical-line hazards
/// remain rejected until they have separate typed semantics.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SayMessage(Box<str>);

impl SayMessage {
    #[cfg(test)]
    pub(super) fn from_unchecked(message: &str) -> Self {
        Self(message.into())
    }

    /// Validates a message against the only current Java target.
    ///
    /// # Errors
    ///
    /// Rejects text outside the initial literal-message subset or beyond the
    /// native target bound.
    pub fn new(message: &str) -> Result<Self, SayMessageError> {
        Self::new_for_target(message, JavaEditionTarget::V26_2)
    }

    /// Validates a message against an explicit Java target.
    ///
    /// # Errors
    ///
    /// Rejects text outside the initial literal-message subset or beyond the
    /// selected target's native bound.
    pub fn new_for_target(
        message: &str,
        target: JavaEditionTarget,
    ) -> Result<Self, SayMessageError> {
        validate_message(message, target)?;
        Ok(Self(message.into()))
    }

    /// Selects a target representation for an already validated semantic literal.
    ///
    /// This is the compiler lowering boundary: target-independent validation has
    /// already happened in [`MessageLiteral`], while physical-line and native
    /// version limits remain checked here.
    ///
    /// # Errors
    ///
    /// Returns a selected-target representation failure.
    pub fn from_message_literal(
        message: &MessageLiteral,
        target: JavaEditionTarget,
    ) -> Result<Self, SayMessageError> {
        Self::new_for_target(message.as_str(), target)
    }

    /// Returns the literal message text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One typed native `say` command.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SayCommand {
    message: SayMessage,
}

impl SayCommand {
    /// Constructs a command from an already validated literal message.
    #[must_use]
    pub const fn new(message: SayMessage) -> Self {
        Self { message }
    }

    /// Returns the validated literal message.
    #[must_use]
    pub const fn message(&self) -> &SayMessage {
        &self.message
    }
}

fn validate_message(message: &str, target: JavaEditionTarget) -> Result<(), SayMessageError> {
    if message.is_empty() {
        return Err(SayMessageError::Empty);
    }
    if message.starts_with(char::is_whitespace) || message.ends_with(char::is_whitespace) {
        return Err(SayMessageError::BoundaryWhitespace);
    }
    for (index, character) in message.char_indices() {
        if character.is_control() {
            return Err(SayMessageError::ControlCharacter { index, character });
        }
        if character == '@' {
            return Err(SayMessageError::SelectorInterpolation { index });
        }
    }
    if message.ends_with('\\') {
        return Err(SayMessageError::TerminalContinuation);
    }
    let units = message.encode_utf16().count();
    let max = match target {
        JavaEditionTarget::V26_2 => MAX_SAY_MESSAGE_UTF16_UNITS,
    };
    if units > max {
        return Err(SayMessageError::TooLong { units, max });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_SAY_MESSAGE_UTF16_UNITS, SayCommand, SayMessage, SayMessageError};

    #[test]
    fn literal_subset_preserves_internal_text_exactly() {
        let message = SayMessage::new("hello  π 😀 \\\"quoted\\\"").unwrap();
        let command = SayCommand::new(message);

        assert_eq!(command.message().as_str(), "hello  π 😀 \\\"quoted\\\"");
    }

    #[test]
    fn literal_subset_rejects_ambiguous_or_physical_line_forms() {
        for (message, expected) in [
            ("", SayMessageError::Empty),
            (" leading", SayMessageError::BoundaryWhitespace),
            ("trailing ", SayMessageError::BoundaryWhitespace),
            (
                "line\nbreak",
                SayMessageError::ControlCharacter {
                    index: 4,
                    character: '\n',
                },
            ),
            (
                "hello @s",
                SayMessageError::SelectorInterpolation { index: 6 },
            ),
            ("continued\\", SayMessageError::TerminalContinuation),
        ] {
            assert_eq!(SayMessage::new(message), Err(expected));
        }
    }

    #[test]
    fn native_bound_counts_java_utf16_units() {
        let exact = "a".repeat(MAX_SAY_MESSAGE_UTF16_UNITS);
        assert!(SayMessage::new(&exact).is_ok());

        let over = format!("{}😀", "a".repeat(MAX_SAY_MESSAGE_UTF16_UNITS - 1));
        assert_eq!(
            SayMessage::new(&over),
            Err(SayMessageError::TooLong {
                units: MAX_SAY_MESSAGE_UTF16_UNITS + 1,
                max: MAX_SAY_MESSAGE_UTF16_UNITS,
            })
        );
    }
}
