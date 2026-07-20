use std::error::Error;
use std::fmt;

use crate::ir::command_line::{CommandLineShapeError, validate_command_line_shape};
use crate::source::OriginId;

use super::StoragePath;

/// Where a runtime value can safely appear in command syntax.
///
/// Each variant represents a distinct syntax position into which a macro variable
/// `$(key)` may be substituted. The compiler owns the serialization, escaping,
/// and validation rules for every slot.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MacroSlot {
    /// Plain integer (no type suffix). Numeric NBT → decimal text.
    Int,
    /// Floating-point (up to 15 fraction digits). Float/Double NBT → decimal text.
    Float,
    /// Canonical SNBT representation. Compound, List, Array NBT types.
    Snbt,
    /// NBT compound key (quoted if needed for special characters).
    NbtKey,
    /// Integer array or list index. Validated non-negative, within bounds where known.
    NbtIndex,
    /// Resource location identifier (`namespace:path`).
    ResourceId,
    /// Target selector fragment.
    SelectorFragment,
    /// Raw command text fragment. **Unsafe** — requires explicit opt-in.
    /// Never produced by automatic lowering.
    CommandFragment,
}

/// One segment of a macro command line.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MacroSegment {
    /// Literal command text that is not substituted.
    Literal(String),
    /// A substituted value reference — renders as `$(key)`.
    Variable(MacroVariableId),
}

/// Opaque identifier for a variable within one macro command.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct MacroVariableId(pub u16);

/// One named variable in a macro argument frame.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MacroVariable {
    /// The `$(key)` identifier. Must match the regex `[a-zA-Z0-9_]+`.
    pub key: String,
    /// What syntax position this variable targets.
    pub slot: MacroSlot,
    /// Where the runtime value lives in command storage.
    pub source: StoragePath,
}

/// The NBT compound template that carries values into a macro function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacroArguments {
    /// Variables in declaration order. Keys must be unique within the frame.
    pub variables: Vec<MacroVariable>,
}

impl MacroArguments {
    /// Constructs a validated argument frame.
    ///
    /// # Errors
    ///
    /// Returns `MacroArgumentsError` if any key is empty, invalid, or
    /// duplicated.
    pub fn new(variables: Vec<MacroVariable>) -> Result<Self, MacroArgumentsError> {
        for (index, variable) in variables.iter().enumerate() {
            validate_variable_key(&variable.key).map_err(|error| MacroArgumentsError {
                index,
                key: variable.key.clone(),
                reason: MacroArgumentsErrorReason::InvalidKey(error),
            })?;
            for (other_index, other) in variables[..index].iter().enumerate() {
                if variable.key == other.key {
                    return Err(MacroArgumentsError {
                        index,
                        key: variable.key.clone(),
                        reason: MacroArgumentsErrorReason::Duplicate { first: other_index },
                    });
                }
            }
        }
        Ok(Self { variables })
    }

    /// Returns the number of variables.
    #[must_use]
    pub fn len(&self) -> usize {
        self.variables.len()
    }

    /// Returns whether there are no variables.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }

    /// Looks up a variable by id.
    #[must_use]
    pub fn get(&self, id: MacroVariableId) -> Option<&MacroVariable> {
        self.variables.get(usize::from(id.0))
    }
}

/// A macro function command — one or more command lines, some of which contain
/// `$(variable)` substitution.
#[derive(Clone, Debug, PartialEq)]
pub struct MacroCommand {
    /// The segments of the macro body. Segments are grouped into lines separated
    /// by command boundaries: segments before the first `CommandBreak` form the
    /// first line, and so on.
    pub lines: Vec<MacroLine>,
    /// The argument frame definition.
    pub arguments: MacroArguments,
    /// Source provenance.
    pub origin: OriginId,
}

/// One command line within a macro command.
///
/// A line whose segments contain at least one `MacroSegment::Variable` is
/// rendered with a `$` prefix. A line containing only `MacroSegment::Literal`
/// segments is rendered without `$` (it does not need substitution, so it
/// benefits from load-time parsing).
#[derive(Clone, Debug, PartialEq)]
pub struct MacroLine {
    pub segments: Vec<MacroSegment>,
}

impl MacroLine {
    /// Returns true if this line contains at least one variable reference.
    #[must_use]
    pub fn has_variables(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, MacroSegment::Variable(_)))
    }
}

impl MacroCommand {
    /// Constructs a macro command.
    ///
    /// # Errors
    ///
    /// Rejects an empty line list.
    pub fn new(
        lines: Vec<MacroLine>,
        arguments: MacroArguments,
        origin: OriginId,
    ) -> Result<Self, MacroCommandError> {
        if lines.is_empty() {
            return Err(MacroCommandError::NoLines);
        }
        for (index, line) in lines.iter().enumerate() {
            if line.segments.is_empty() {
                return Err(MacroCommandError::EmptyLine { index });
            }
            // Physical shape is a property of the whole assembled command line, not of
            // each literal fragment: a literal that precedes a substitution (e.g.
            // `say `) legitimately ends in whitespace. Assemble the line — literals
            // verbatim, variables as their `$(key)` placeholders — then validate once.
            let mut assembled = String::new();
            for segment in &line.segments {
                match segment {
                    MacroSegment::Literal(text) => assembled.push_str(text),
                    MacroSegment::Variable(id) => {
                        let variable =
                            arguments
                                .get(*id)
                                .ok_or(MacroCommandError::UnknownVariable {
                                    line: index,
                                    variable: *id,
                                })?;
                        assembled.push_str("$(");
                        assembled.push_str(&variable.key);
                        assembled.push(')');
                    }
                }
            }
            if let Err(hazard) = validate_command_line_shape(&assembled) {
                // A macro line is physically emitted with a leading `$`, so an assembled
                // line that begins with a `$(variable)` substitution is legitimate even
                // though its command text starts with `$`.
                let starts_with_substitution = hazard == CommandLineShapeError::ReservedPrefix('$')
                    && assembled.starts_with("$(");
                if !starts_with_substitution {
                    return Err(MacroCommandError::LiteralHazard {
                        line: index,
                        reason: hazard.to_string(),
                    });
                }
            }
        }
        Ok(Self {
            lines,
            arguments,
            origin,
        })
    }

    /// Returns the source provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

/// Why a macro command was rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MacroCommandError {
    /// No lines were supplied.
    NoLines,
    /// A line has no segments.
    EmptyLine { index: usize },
    /// A line references a variable id absent from the argument frame.
    UnknownVariable {
        line: usize,
        variable: MacroVariableId,
    },
    /// The assembled command line violates physical-line constraints.
    LiteralHazard { line: usize, reason: String },
}

impl fmt::Display for MacroCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLines => formatter.write_str("macro command must have at least one line"),
            Self::EmptyLine { index } => {
                write!(formatter, "macro line {index} has no segments")
            }
            Self::UnknownVariable { line, variable } => {
                write!(
                    formatter,
                    "macro line {line} references unknown variable {}",
                    variable.0
                )
            }
            Self::LiteralHazard { line, reason } => {
                write!(formatter, "assembled macro line {line}: {reason}")
            }
        }
    }
}

impl Error for MacroCommandError {}

/// Why a macro argument frame was rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacroArgumentsError {
    /// Index of the variable that triggered the error.
    pub index: usize,
    /// The variable key involved.
    pub key: String,
    /// Why the frame was rejected.
    pub reason: MacroArgumentsErrorReason,
}

impl fmt::Display for MacroArgumentsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reason {
            MacroArgumentsErrorReason::InvalidKey(error) => {
                write!(
                    formatter,
                    "macro variable {:?} (index {}): {error}",
                    self.key, self.index
                )
            }
            MacroArgumentsErrorReason::Duplicate { first } => {
                write!(
                    formatter,
                    "macro variable {:?} (index {}) duplicates index {first}",
                    self.key, self.index
                )
            }
        }
    }
}

impl Error for MacroArgumentsError {}

/// Why a macro argument frame was rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MacroArgumentsErrorReason {
    /// The variable key is syntactically invalid.
    InvalidKey(MacroKeyError),
    /// The variable key appears earlier in the frame.
    Duplicate { first: usize },
}

/// Why a variable key was rejected.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MacroKeyError {
    /// Key is empty.
    Empty,
    /// Key contains a character outside `[a-zA-Z0-9_]`.
    InvalidCharacter,
}

impl fmt::Display for MacroKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("macro variable key must not be empty"),
            Self::InvalidCharacter => formatter.write_str(
                "macro variable key must contain only ASCII letters, digits, and underscores",
            ),
        }
    }
}

impl Error for MacroKeyError {}

fn validate_variable_key(key: &str) -> Result<(), MacroKeyError> {
    if key.is_empty() {
        return Err(MacroKeyError::Empty);
    }
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(MacroKeyError::InvalidCharacter);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MacroArguments, MacroArgumentsErrorReason, MacroCommand, MacroCommandError, MacroKeyError,
        MacroLine, MacroSegment, MacroSlot, MacroVariable, MacroVariableId,
    };
    use crate::ir::minecraft::{NbtPath, NbtPathKey, NbtPathSegment, StorageId, StoragePath};
    use crate::source::OriginId;

    fn storage(key: &str) -> StoragePath {
        StoragePath::new(
            StorageId::parse("mdl:st").unwrap(),
            NbtPath::new(NbtPathSegment::Key(NbtPathKey::new(key).unwrap()), vec![]),
        )
    }

    fn var(_id: u16, key: &str, slot: MacroSlot) -> MacroVariable {
        MacroVariable {
            key: key.to_owned(),
            slot,
            // The storage source key is independent of the macro variable key under
            // test, so it stays a fixed valid path even when `key` is empty/invalid.
            source: storage("src"),
        }
    }

    fn vid(id: u16) -> MacroVariableId {
        MacroVariableId(id)
    }

    #[test]
    fn variable_keys_must_be_alphanumeric_or_underscore() {
        for (key, expected) in [
            ("", Some(MacroKeyError::Empty)),
            ("valid_key", None),
            ("mixedCase123", None),
            ("key with space", Some(MacroKeyError::InvalidCharacter)),
            ("key-with-dash", Some(MacroKeyError::InvalidCharacter)),
        ] {
            let result = MacroArguments::new(vec![var(0, key, MacroSlot::Int)]).map(|_| ());
            match (result, expected) {
                (Ok(()), None) => {}
                (Err(error), Some(key_error))
                    if error.reason == MacroArgumentsErrorReason::InvalidKey(key_error) => {}
                (result, expected) => panic!("{key:?}: got {result:?}, expected {expected:?}"),
            }
        }
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let error = MacroArguments::new(vec![
            var(0, "a", MacroSlot::Int),
            var(1, "a", MacroSlot::Float),
        ])
        .unwrap_err();
        assert_eq!(
            error.reason,
            MacroArgumentsErrorReason::Duplicate { first: 0 }
        );
        assert_eq!(error.index, 1);
    }

    #[test]
    fn macro_command_rejects_empty_and_hazardous_literals() {
        assert_eq!(
            MacroCommand::new(
                vec![],
                MacroArguments::new(vec![]).unwrap(),
                OriginId::UNKNOWN
            )
            .unwrap_err(),
            MacroCommandError::NoLines
        );
        assert_eq!(
            MacroCommand::new(
                vec![MacroLine { segments: vec![] }],
                MacroArguments::new(vec![]).unwrap(),
                OriginId::UNKNOWN
            )
            .unwrap_err(),
            MacroCommandError::EmptyLine { index: 0 }
        );
        assert!(matches!(
            MacroCommand::new(
                vec![MacroLine {
                    segments: vec![MacroSegment::Literal("say\nhi".to_owned())],
                }],
                MacroArguments::new(vec![]).unwrap(),
                OriginId::UNKNOWN
            )
            .unwrap_err(),
            MacroCommandError::LiteralHazard { .. }
        ));
    }

    #[test]
    fn has_variables_detects_substitution() {
        let pure = MacroLine {
            segments: vec![MacroSegment::Literal("say hi".to_owned())],
        };
        assert!(!pure.has_variables());
        let mixed = MacroLine {
            segments: vec![
                MacroSegment::Literal("say ".to_owned()),
                MacroSegment::Variable(vid(0)),
            ],
        };
        assert!(mixed.has_variables());
    }

    #[test]
    fn valid_macro_command_is_accepted() {
        let args = MacroArguments::new(vec![var(0, "msg", MacroSlot::Int)]).unwrap();
        let cmd = MacroCommand::new(
            vec![MacroLine {
                segments: vec![
                    MacroSegment::Literal("say ".to_owned()),
                    MacroSegment::Variable(vid(0)),
                ],
            }],
            args,
            OriginId::UNKNOWN,
        );
        assert!(cmd.is_ok());
    }
}
