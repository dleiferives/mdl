use std::error::Error;
use std::fmt;
use std::fmt::Write as _;

use super::{FiniteF32, FiniteF64, StorageId};
use crate::ir::core::Operand;

/// Initial maximum nesting depth for compiler-constructed NBT.
pub const MAX_NBT_DEPTH: usize = 64;

/// An arbitrary NBT compound key, including the empty string.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NbtKey(Box<str>);

impl NbtKey {
    /// Owns an NBT compound key without restricting its contents.
    #[must_use]
    pub fn new(value: &str) -> Self {
        Self(value.into())
    }

    /// Returns the key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for NbtKey {
    fn from(value: String) -> Self {
        Self(value.into_boxed_str())
    }
}

impl From<&str> for NbtKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Why an NBT path key was rejected.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NbtPathKeyErrorReason {
    /// NBT traversal does not accept an empty key.
    Empty,
    /// Physical command-safe path syntax does not admit this control character.
    ControlCharacter {
        /// Byte offset within the key.
        byte_index: usize,
        /// Rejected Unicode control character.
        character: char,
    },
}

/// A failed NBT path-key validation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NbtPathKeyError(NbtPathKeyErrorReason);

impl NbtPathKeyError {
    /// Returns the precise rejection reason.
    #[must_use]
    pub const fn reason(self) -> NbtPathKeyErrorReason {
        self.0
    }
}

impl fmt::Display for NbtPathKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            NbtPathKeyErrorReason::Empty => formatter.write_str("NBT path key cannot be empty"),
            NbtPathKeyErrorReason::ControlCharacter {
                byte_index,
                character,
            } => write!(
                formatter,
                "NBT path key contains control character {character:?} at byte {byte_index}"
            ),
        }
    }
}

impl Error for NbtPathKeyError {}

/// A nonempty, physical-command-safe NBT traversal key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NbtPathKey(NbtKey);

impl NbtPathKey {
    /// Validates and owns an NBT path key.
    ///
    /// # Errors
    ///
    /// Rejects an empty value or a Unicode control character.
    pub fn new(value: &str) -> Result<Self, NbtPathKeyError> {
        Self::try_from(NbtKey::new(value))
    }

    /// Returns the path-key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<NbtKey> for NbtPathKey {
    type Error = NbtPathKeyError;

    fn try_from(key: NbtKey) -> Result<Self, Self::Error> {
        validate_path_key(key.as_str())?;
        Ok(Self(key))
    }
}

impl TryFrom<String> for NbtPathKey {
    type Error = NbtPathKeyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(NbtKey::from(value))
    }
}

impl From<NbtPathKey> for NbtKey {
    fn from(key: NbtPathKey) -> Self {
        key.0
    }
}

/// One statically known NBT traversal operation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum NbtPathSegment {
    /// Select a compound child.
    Key(NbtPathKey),
    /// Select one list element; negative indexes count from the end.
    ///
    /// A `Const` index renders as `[n]`. A `Runtime` index represents a
    /// dynamic value that must be macro-substituted; its `$(key)` rendering
    /// is produced by `extract_crossings` (PS-11C) where the macro frame
    /// context is available.
    Index(Operand<i32>),
    /// Select every element of a list.
    AllElements,
}

/// An attempted empty static NBT path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EmptyNbtPath;

impl fmt::Display for EmptyNbtPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NBT path must contain at least one segment")
    }
}

impl Error for EmptyNbtPath {}

/// A nonempty static NBT traversal path.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct NbtPath {
    segments: Box<[NbtPathSegment]>,
}

impl NbtPath {
    /// Constructs a path from a required first segment and ordered remainder.
    #[must_use]
    pub fn new(first: NbtPathSegment, remainder: Vec<NbtPathSegment>) -> Self {
        let mut segments = Vec::with_capacity(remainder.len() + 1);
        segments.push(first);
        segments.extend(remainder);
        Self {
            segments: segments.into_boxed_slice(),
        }
    }

    /// Validates a pre-collected path sequence.
    ///
    /// # Errors
    ///
    /// Rejects an empty sequence.
    pub fn from_segments(segments: Vec<NbtPathSegment>) -> Result<Self, EmptyNbtPath> {
        if segments.is_empty() {
            Err(EmptyNbtPath)
        } else {
            Ok(Self {
                segments: segments.into_boxed_slice(),
            })
        }
    }

    /// Returns the path segments in target order.
    #[must_use]
    pub fn segments(&self) -> &[NbtPathSegment] {
        &self.segments
    }
}

impl fmt::Display for NbtPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, segment) in self.segments.iter().enumerate() {
            match segment {
                NbtPathSegment::Key(key) => {
                    if index != 0 {
                        formatter.write_char('.')?;
                    }
                    write_path_key(key.as_str(), formatter)?;
                }
                NbtPathSegment::Index(element) => match element {
                    Operand::Const(n) => write!(formatter, "[{n}]")?,
                    Operand::Runtime(_) => panic!(
                        "runtime NBT index rendered outside a macro context; \
                         PS-11C extract_crossings handles $(key) substitution"
                    ),
                },
                NbtPathSegment::AllElements => formatter.write_str("[]")?,
            }
        }
        Ok(())
    }
}

/// One storage resource and a nonempty static path within it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StoragePath {
    storage: StorageId,
    path: NbtPath,
}

impl StoragePath {
    /// Constructs a static storage path.
    #[must_use]
    pub const fn new(storage: StorageId, path: NbtPath) -> Self {
        Self { storage, path }
    }

    /// Returns the storage resource.
    #[must_use]
    pub const fn storage(&self) -> &StorageId {
        &self.storage
    }

    /// Returns the static traversal path.
    #[must_use]
    pub const fn path(&self) -> &NbtPath {
        &self.path
    }
}

/// A borrowed view of one immutable NBT value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NbtValueRef<'a> {
    /// Signed byte.
    Byte(i8),
    /// Signed short.
    Short(i16),
    /// Signed integer.
    Int(i32),
    /// Signed long.
    Long(i64),
    /// Finite float.
    Float(FiniteF32),
    /// Finite double.
    Double(FiniteF64),
    /// UTF-8 Rust string.
    String(&'a str),
    /// Ordered heterogeneous list.
    List(&'a [NbtValue]),
    /// Key-sorted compound entries.
    Compound(&'a [(NbtKey, NbtValue)]),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum NbtValueKind {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(FiniteF32),
    Double(FiniteF64),
    String(Box<str>),
    List(Vec<NbtValue>),
    Compound(Vec<(NbtKey, NbtValue)>),
}

/// An immutable, canonicalizable Stage 3 NBT value.
#[derive(Clone, Debug, PartialEq)]
pub struct NbtValue {
    pub(super) kind: NbtValueKind,
}

impl NbtValue {
    /// Constructs a byte tag.
    #[must_use]
    pub const fn byte(value: i8) -> Self {
        Self {
            kind: NbtValueKind::Byte(value),
        }
    }

    /// Constructs a short tag.
    #[must_use]
    pub const fn short(value: i16) -> Self {
        Self {
            kind: NbtValueKind::Short(value),
        }
    }

    /// Constructs an integer tag.
    #[must_use]
    pub const fn int(value: i32) -> Self {
        Self {
            kind: NbtValueKind::Int(value),
        }
    }

    /// Constructs a long tag.
    #[must_use]
    pub const fn long(value: i64) -> Self {
        Self {
            kind: NbtValueKind::Long(value),
        }
    }

    /// Constructs a finite float tag.
    #[must_use]
    pub const fn float(value: FiniteF32) -> Self {
        Self {
            kind: NbtValueKind::Float(value),
        }
    }

    /// Constructs a finite double tag.
    #[must_use]
    pub const fn double(value: FiniteF64) -> Self {
        Self {
            kind: NbtValueKind::Double(value),
        }
    }

    /// Constructs a string tag.
    #[must_use]
    pub fn string(value: &str) -> Self {
        Self {
            kind: NbtValueKind::String(value.into()),
        }
    }

    /// Constructs a string tag without copying an owned string.
    #[must_use]
    pub fn string_owned(value: String) -> Self {
        Self {
            kind: NbtValueKind::String(value.into_boxed_str()),
        }
    }

    /// Constructs an ordered heterogeneous list.
    ///
    /// # Errors
    ///
    /// Rejects a resulting nesting depth greater than [`MAX_NBT_DEPTH`].
    pub fn list(values: Vec<Self>) -> Result<Self, NbtBuildError> {
        let value = Self {
            kind: NbtValueKind::List(values),
        };
        value.validate_depth()?;
        Ok(value)
    }

    /// Constructs a key-sorted compound with unique keys.
    ///
    /// # Errors
    ///
    /// Rejects duplicate keys or a resulting nesting depth greater than
    /// [`MAX_NBT_DEPTH`].
    pub fn compound(mut entries: Vec<(NbtKey, Self)>) -> Result<Self, NbtBuildError> {
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        if let Some(duplicate) = entries.windows(2).find(|window| window[0].0 == window[1].0) {
            return Err(NbtBuildError::DuplicateKey(duplicate[0].0.clone()));
        }
        let value = Self {
            kind: NbtValueKind::Compound(entries),
        };
        value.validate_depth()?;
        Ok(value)
    }

    /// Returns a borrowed view of this value.
    #[must_use]
    pub fn as_ref(&self) -> NbtValueRef<'_> {
        match &self.kind {
            NbtValueKind::Byte(value) => NbtValueRef::Byte(*value),
            NbtValueKind::Short(value) => NbtValueRef::Short(*value),
            NbtValueKind::Int(value) => NbtValueRef::Int(*value),
            NbtValueKind::Long(value) => NbtValueRef::Long(*value),
            NbtValueKind::Float(value) => NbtValueRef::Float(*value),
            NbtValueKind::Double(value) => NbtValueRef::Double(*value),
            NbtValueKind::String(value) => NbtValueRef::String(value),
            NbtValueKind::List(values) => NbtValueRef::List(values),
            NbtValueKind::Compound(entries) => NbtValueRef::Compound(entries),
        }
    }

    /// Returns the structural nesting depth, where every scalar has depth one.
    #[must_use]
    pub fn depth(&self) -> usize {
        match &self.kind {
            NbtValueKind::List(values) => 1 + values.iter().map(Self::depth).max().unwrap_or(0),
            NbtValueKind::Compound(entries) => {
                1 + entries
                    .iter()
                    .map(|(_, value)| value.depth())
                    .max()
                    .unwrap_or(0)
            }
            NbtValueKind::Byte(_)
            | NbtValueKind::Short(_)
            | NbtValueKind::Int(_)
            | NbtValueKind::Long(_)
            | NbtValueKind::Float(_)
            | NbtValueKind::Double(_)
            | NbtValueKind::String(_) => 1,
        }
    }

    fn validate_depth(&self) -> Result<(), NbtBuildError> {
        let depth = self.depth();
        if depth > MAX_NBT_DEPTH {
            Err(NbtBuildError::DepthExceeded {
                depth,
                max: MAX_NBT_DEPTH,
            })
        } else {
            Ok(())
        }
    }

    pub(super) fn write_snbt(&self, output: &mut impl fmt::Write) -> fmt::Result {
        match &self.kind {
            NbtValueKind::Byte(value) => write!(output, "{value}b"),
            NbtValueKind::Short(value) => write!(output, "{value}s"),
            NbtValueKind::Int(value) => write!(output, "{value}"),
            NbtValueKind::Long(value) => write!(output, "{value}L"),
            NbtValueKind::Float(value) => write!(output, "{value}f"),
            NbtValueKind::Double(value) => write!(output, "{value}d"),
            NbtValueKind::String(value) => write_snbt_string(value, output),
            NbtValueKind::List(values) => {
                output.write_char('[')?;
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.write_char(',')?;
                    }
                    value.write_snbt(output)?;
                }
                output.write_char(']')
            }
            NbtValueKind::Compound(entries) => {
                output.write_char('{')?;
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index != 0 {
                        output.write_char(',')?;
                    }
                    write_snbt_string(key.as_str(), output)?;
                    output.write_char(':')?;
                    value.write_snbt(output)?;
                }
                output.write_char('}')
            }
        }
    }
}

impl fmt::Display for NbtValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_snbt(formatter)
    }
}

/// A failed immutable NBT construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NbtBuildError {
    /// Two compound entries used the same exact key.
    DuplicateKey(NbtKey),
    /// A list or compound exceeded the compiler structural-depth limit.
    DepthExceeded {
        /// Observed depth.
        depth: usize,
        /// Configured compiler maximum.
        max: usize,
    },
}

impl fmt::Display for NbtBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateKey(key) => {
                write!(formatter, "duplicate NBT compound key {:?}", key.as_str())
            }
            Self::DepthExceeded { depth, max } => {
                write!(formatter, "NBT depth {depth} exceeds compiler limit {max}")
            }
        }
    }
}

impl Error for NbtBuildError {}

fn validate_path_key(value: &str) -> Result<(), NbtPathKeyError> {
    if value.is_empty() {
        return Err(NbtPathKeyError(NbtPathKeyErrorReason::Empty));
    }
    if let Some((byte_index, character)) = value
        .char_indices()
        .find(|(_, character)| character.is_control())
    {
        return Err(NbtPathKeyError(NbtPathKeyErrorReason::ControlCharacter {
            byte_index,
            character,
        }));
    }
    Ok(())
}

fn write_path_key(value: &str, output: &mut impl fmt::Write) -> fmt::Result {
    output.write_char('"')?;
    for character in value.chars() {
        match character {
            '"' => output.write_str("\\\"")?,
            '\\' => output.write_str("\\\\")?,
            _ => output.write_char(character)?,
        }
    }
    output.write_char('"')
}

fn write_snbt_string(value: &str, output: &mut impl fmt::Write) -> fmt::Result {
    output.write_char('"')?;
    for character in value.chars() {
        match character {
            '"' => output.write_str("\\\"")?,
            '\\' => output.write_str("\\\\")?,
            '\u{0008}' => output.write_str("\\b")?,
            '\t' => output.write_str("\\t")?,
            '\n' => output.write_str("\\n")?,
            '\u{000c}' => output.write_str("\\f")?,
            '\r' => output.write_str("\\r")?,
            control if control < '\u{0020}' => write!(output, "\\x{:02x}", u32::from(control))?,
            _ => output.write_char(character)?,
        }
    }
    output.write_char('"')
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_NBT_DEPTH, NbtBuildError, NbtKey, NbtPath, NbtPathKey, NbtPathKeyErrorReason,
        NbtPathSegment, NbtValue, NbtValueRef, StoragePath,
    };
    use crate::ir::core::Operand;
    use crate::ir::minecraft::{FiniteF32, FiniteF64, StorageId};

    #[test]
    fn compound_keys_and_path_keys_have_distinct_domains() {
        assert_eq!(NbtKey::new("").as_str(), "");
        assert_eq!(
            NbtPathKey::new("").unwrap_err().reason(),
            NbtPathKeyErrorReason::Empty
        );
        assert_eq!(NbtPathKey::new("a.b").unwrap().as_str(), "a.b");
        assert_eq!(NbtPathKey::new("a b").unwrap().as_str(), "a b");
        assert_eq!(
            NbtPathKey::new("a\nb").unwrap_err().reason(),
            NbtPathKeyErrorReason::ControlCharacter {
                byte_index: 1,
                character: '\n'
            }
        );

        let key = NbtPathKey::new("quoted\"\\key").unwrap();
        let compound_key: NbtKey = key.into();
        assert_eq!(compound_key.as_str(), "quoted\"\\key");
    }

    #[test]
    fn static_paths_are_nonempty_ordered_and_canonically_quoted() {
        let path = NbtPath::new(
            NbtPathSegment::Key(NbtPathKey::new("prison.cells").unwrap()),
            vec![
                NbtPathSegment::Index(Operand::Const(-1)),
                NbtPathSegment::AllElements,
                NbtPathSegment::Key(NbtPathKey::new("user \"name\"").unwrap()),
            ],
        );
        assert_eq!(
            path.to_string(),
            "\"prison.cells\"[-1][].\"user \\\"name\\\"\""
        );
        assert!(NbtPath::from_segments(vec![]).is_err());

        let storage = StorageId::parse("mdl:runtime").unwrap();
        let storage_path = StoragePath::new(storage, path);
        assert_eq!(storage_path.storage().to_string(), "mdl:runtime");
        assert_eq!(storage_path.path().segments().len(), 4);
    }

    #[test]
    fn every_scalar_has_exact_canonical_snbt() {
        let values = [
            (NbtValue::byte(i8::MIN), "-128b"),
            (NbtValue::short(i16::MAX), "32767s"),
            (NbtValue::int(i32::MIN), "-2147483648"),
            (NbtValue::long(i64::MAX), "9223372036854775807L"),
            (NbtValue::float(FiniteF32::new(-0.0).unwrap()), "-0f"),
            (NbtValue::double(FiniteF64::new(1.5).unwrap()), "1.5d"),
        ];
        for (value, expected) in values {
            assert_eq!(value.to_string(), expected);
            assert_eq!(value.depth(), 1);
        }
    }

    #[test]
    fn snbt_strings_escape_quotes_slashes_controls_and_preserve_unicode() {
        let value = NbtValue::string("quote=\" slash=\\ controls=\0\u{0008}\t\n\u{000c}\r snow=☃");
        assert_eq!(
            value.to_string(),
            "\"quote=\\\" slash=\\\\ controls=\\x00\\b\\t\\n\\f\\r snow=☃\""
        );
    }

    #[test]
    fn lists_are_heterogeneous_and_compounds_are_sorted_unique() {
        let list = NbtValue::list(vec![NbtValue::int(1), NbtValue::string("two")]).unwrap();
        assert_eq!(list.to_string(), "[1,\"two\"]");
        assert!(matches!(list.as_ref(), NbtValueRef::List(values) if values.len() == 2));
        assert_eq!(NbtValue::list(vec![]).unwrap().to_string(), "[]");

        let compound = NbtValue::compound(vec![
            (NbtKey::new("z"), NbtValue::int(1)),
            (NbtKey::new(""), NbtValue::byte(2)),
            (NbtKey::new("a"), NbtValue::string("x")),
        ])
        .unwrap();
        assert_eq!(compound.to_string(), "{\"\":2b,\"a\":\"x\",\"z\":1}");

        let duplicate = NbtValue::compound(vec![
            (NbtKey::new("same"), NbtValue::int(1)),
            (NbtKey::new("same"), NbtValue::int(2)),
        ])
        .unwrap_err();
        assert_eq!(duplicate, NbtBuildError::DuplicateKey(NbtKey::new("same")));
    }

    #[test]
    fn depth_64_is_valid_and_depth_65_is_rejected() {
        let mut value = NbtValue::int(0);
        for _ in 1..MAX_NBT_DEPTH {
            value = NbtValue::list(vec![value]).unwrap();
        }
        assert_eq!(value.depth(), MAX_NBT_DEPTH);

        let error = NbtValue::list(vec![value]).unwrap_err();
        assert_eq!(
            error,
            NbtBuildError::DepthExceeded {
                depth: MAX_NBT_DEPTH + 1,
                max: MAX_NBT_DEPTH
            }
        );
    }
}
