//! Owned, target-independent Minecraft message literals.

use std::error::Error;
use std::fmt;

/// An owned message accepted by MDL's target-independent semantic layer.
///
/// This type deliberately does not enforce a Minecraft-version length limit or
/// reject a terminal backslash. Those are properties of a selected target recipe,
/// not of the source operation's meaning.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MessageLiteral(Box<str>);

impl MessageLiteral {
    /// Validates and takes ownership of one message.
    ///
    /// # Errors
    ///
    /// Rejects empty messages, Unicode control characters, and `@`. The latter is
    /// reserved until Minecraft selector interpolation has typed semantics.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, MessageLiteralError> {
        let value = value.into();
        if value.is_empty() {
            return Err(MessageLiteralError::Empty);
        }
        for (byte_index, character) in value.char_indices() {
            if character.is_control() {
                return Err(MessageLiteralError::ControlCharacter {
                    byte_index,
                    character,
                });
            }
            if character == '@' {
                return Err(MessageLiteralError::SelectorInterpolation { byte_index });
            }
        }
        Ok(Self(value))
    }

    /// Returns the exact validated message text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the exact owned message text.
    #[must_use]
    pub fn into_boxed_str(self) -> Box<str> {
        self.0
    }
}

impl AsRef<str> for MessageLiteral {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl TryFrom<&str> for MessageLiteral {
    type Error = MessageLiteralError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for MessageLiteral {
    type Error = MessageLiteralError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Why a string cannot be a target-independent [`MessageLiteral`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageLiteralError {
    /// The message contains no characters.
    Empty,
    /// A Unicode control character occurs at `byte_index`.
    ControlCharacter {
        /// UTF-8 byte offset of the rejected character.
        byte_index: usize,
        /// Rejected Unicode scalar value.
        character: char,
    },
    /// A reserved Minecraft selector interpolation marker occurs at `byte_index`.
    SelectorInterpolation {
        /// UTF-8 byte offset of the rejected `@`.
        byte_index: usize,
    },
}

impl fmt::Display for MessageLiteralError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("Minecraft messages cannot be empty"),
            Self::ControlCharacter {
                byte_index,
                character,
            } => write!(
                formatter,
                "Minecraft messages cannot contain control character {character:?} at byte {byte_index}"
            ),
            Self::SelectorInterpolation { byte_index } => write!(
                formatter,
                "Minecraft selector interpolation with `@` is not supported at byte {byte_index}"
            ),
        }
    }
}

impl Error for MessageLiteralError {}

#[cfg(test)]
mod tests {
    use super::{MessageLiteral, MessageLiteralError};

    #[test]
    fn preserves_accepted_text_exactly() {
        let text = "  quotes: “MDL”, slash: \\, snowman: ☃  ";
        let message = MessageLiteral::new(text).unwrap();

        assert_eq!(message.as_str(), text);
        assert_eq!(message.clone().into_boxed_str().as_ref(), text);
        assert_eq!(MessageLiteral::try_from(text).unwrap(), message);
        assert_eq!(MessageLiteral::try_from(text.to_owned()).unwrap(), message);
    }

    #[test]
    fn rejects_empty_control_characters_and_selector_interpolation() {
        assert_eq!(MessageLiteral::new(""), Err(MessageLiteralError::Empty));
        assert_eq!(
            MessageLiteral::new("snowman ☃\nnext"),
            Err(MessageLiteralError::ControlCharacter {
                byte_index: 11,
                character: '\n',
            })
        );
        assert_eq!(
            MessageLiteral::new("hello @s"),
            Err(MessageLiteralError::SelectorInterpolation { byte_index: 6 })
        );
        assert_eq!(
            MessageLiteral::new("a\u{85}b"),
            Err(MessageLiteralError::ControlCharacter {
                byte_index: 1,
                character: '\u{85}',
            })
        );
    }

    #[test]
    fn target_limits_are_not_part_of_semantic_validation() {
        let over_java_26_2_limit = "x".repeat(257);
        assert_eq!(
            MessageLiteral::new(over_java_26_2_limit.clone())
                .unwrap()
                .as_str(),
            over_java_26_2_limit
        );
        assert_eq!(
            MessageLiteral::new("terminal backslash \\")
                .unwrap()
                .as_str(),
            "terminal backslash \\"
        );
    }

    #[test]
    fn validation_is_linear_for_large_owned_messages_and_reports_byte_offsets() {
        let accepted = "é".repeat(100_000);
        let literal = MessageLiteral::new(accepted.clone()).unwrap();
        assert_eq!(literal.as_str(), accepted);

        let mut rejected = accepted;
        let expected_index = rejected.len();
        rejected.push('@');
        assert_eq!(
            MessageLiteral::new(rejected),
            Err(MessageLiteralError::SelectorInterpolation {
                byte_index: expected_index,
            })
        );
    }
}
