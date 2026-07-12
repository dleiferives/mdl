//! Immutable source files and append-only provenance.

use std::error::Error;
use std::fmt;

use crate::entity::{EntityLimitError, EntityVec, entity_id};

entity_id!(
    /// Identity of one source file within a [`SourceMap`].
    pub struct FileId;
);
entity_id!(
    /// Identity of one provenance record within a [`SourceContext`].
    pub struct OriginId;
);

impl OriginId {
    /// The distinguished unknown origin present in every source context.
    pub const UNKNOWN: Self = Self(0);
}

/// A validated half-open byte range within one source file.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Span {
    file: FileId,
    start: u32,
    end: u32,
}

impl Span {
    /// Returns the file containing this span.
    #[must_use]
    pub const fn file(self) -> FileId {
        self.file
    }

    /// Returns the inclusive UTF-8 byte offset at which this span starts.
    #[must_use]
    pub const fn start(self) -> u32 {
        self.start
    }

    /// Returns the exclusive UTF-8 byte offset at which this span ends.
    #[must_use]
    pub const fn end(self) -> u32 {
        self.end
    }

    /// Returns the length of this span in UTF-8 bytes.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    /// Returns whether this span contains no bytes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A zero-based line and UTF-8 byte column within a source file.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourcePosition {
    /// Zero-based source line.
    pub line: u32,
    /// Zero-based byte offset from the start of the line.
    pub byte_column: u32,
}

/// One immutable source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    name: Box<str>,
    text: Box<str>,
    line_starts: Box<[u32]>,
}

impl SourceFile {
    /// Returns the diagnostic name supplied when the file was added.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the complete UTF-8 source text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Append-only source file storage.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    files: EntityVec<FileId, SourceFile>,
}

impl SourceMap {
    /// Creates an empty source map.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            files: EntityVec::new(),
        }
    }

    /// Returns the number of source files.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Returns whether the source map has no files.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Adds one immutable UTF-8 source file.
    ///
    /// File names are diagnostic hints and need not be unique.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::FileTooLarge`] when the text length cannot be
    /// represented by a `u32` byte offset, or [`SourceError::EntityLimit`] when no
    /// further file ID can be allocated.
    pub fn add_file(
        &mut self,
        name: impl Into<Box<str>>,
        text: impl Into<Box<str>>,
    ) -> Result<FileId, SourceError> {
        let name = name.into();
        let text = text.into();
        let byte_len = text.len();
        u32::try_from(byte_len).map_err(|_| SourceError::FileTooLarge { byte_len })?;

        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                let next =
                    u32::try_from(index + 1).map_err(|_| SourceError::FileTooLarge { byte_len })?;
                line_starts.push(next);
            }
        }

        self.files
            .push(SourceFile {
                name,
                text,
                line_starts: line_starts.into_boxed_slice(),
            })
            .map_err(|_| SourceError::EntityLimit)
    }

    /// Returns a source file, or `None` for an ID outside this map.
    #[must_use]
    pub fn get(&self, file: FileId) -> Option<&SourceFile> {
        self.files.get(file)
    }

    /// Creates a validated half-open source span.
    ///
    /// # Errors
    ///
    /// Returns an error when the file is absent, the range is reversed or outside
    /// the file, or either offset splits a UTF-8 code point.
    pub fn span(&self, file: FileId, start: u32, end: u32) -> Result<Span, SourceError> {
        let source = self.get(file).ok_or(SourceError::InvalidFile { file })?;
        if start > end {
            return Err(SourceError::ReversedSpan { start, end });
        }

        let file_len = u32::try_from(source.text.len()).map_err(|_| SourceError::FileTooLarge {
            byte_len: source.text.len(),
        })?;
        if end > file_len {
            return Err(SourceError::SpanOutOfBounds {
                file,
                end,
                file_len,
            });
        }

        for offset in [start, end] {
            let Ok(offset_usize) = usize::try_from(offset) else {
                return Err(SourceError::SpanOutOfBounds {
                    file,
                    end,
                    file_len,
                });
            };
            if !source.text.is_char_boundary(offset_usize) {
                return Err(SourceError::NotCharBoundary { file, offset });
            }
        }

        Ok(Span { file, start, end })
    }

    /// Resolves a validated byte offset to a zero-based line and byte column.
    ///
    /// An offset at the end of the file is valid. Offsets splitting a UTF-8 code
    /// point are rejected.
    ///
    /// # Errors
    ///
    /// Returns the same validation errors as an empty [`Span`] at `offset`.
    pub fn resolve_position(
        &self,
        file: FileId,
        offset: u32,
    ) -> Result<SourcePosition, SourceError> {
        self.span(file, offset, offset)?;
        let source = self.get(file).ok_or(SourceError::InvalidFile { file })?;
        let after_last_start = source
            .line_starts
            .partition_point(|line_start| *line_start <= offset);
        let line_index = after_last_start - 1;
        let line = u32::try_from(line_index).map_err(|_| SourceError::FileTooLarge {
            byte_len: source.text.len(),
        })?;
        let byte_column = offset - source.line_starts[line_index];
        Ok(SourcePosition { line, byte_column })
    }

    fn validate_span(&self, span: Span) -> Result<(), SourceError> {
        self.span(span.file, span.start, span.end).map(|_| ())
    }
}

/// Immutable provenance attached to compiler entities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Origin {
    /// No source provenance is available.
    Unknown,
    /// A range directly present in a source file.
    Source(Span),
    /// A callee construct expanded or instantiated at a caller location.
    CallSite {
        /// Provenance of the expanded construct.
        callee: OriginId,
        /// Provenance of the expansion site.
        caller: OriginId,
    },
    /// Provenance derived from multiple inputs.
    Fused {
        /// Existing provenance records contributing to this result.
        inputs: Vec<OriginId>,
        /// Optional diagnostic explanation for the fusion.
        reason: Option<Box<str>>,
    },
}

/// Source files and provenance shared by compiler IR levels.
#[derive(Clone, Debug)]
pub struct SourceContext {
    files: SourceMap,
    origins: EntityVec<OriginId, Origin>,
}

impl SourceContext {
    /// Creates an empty context containing [`OriginId::UNKNOWN`] at origin zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: SourceMap::new(),
            origins: EntityVec::with_first(Origin::Unknown),
        }
    }

    /// Returns the immutable source map.
    #[must_use]
    pub const fn files(&self) -> &SourceMap {
        &self.files
    }

    /// Adds one immutable source file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is too large or the file ID space is exhausted.
    pub fn add_file(
        &mut self,
        name: impl Into<Box<str>>,
        text: impl Into<Box<str>>,
    ) -> Result<FileId, SourceError> {
        self.files.add_file(name, text)
    }

    /// Creates a validated source span.
    ///
    /// # Errors
    ///
    /// Returns an error if the file or byte range is invalid.
    pub fn span(&self, file: FileId, start: u32, end: u32) -> Result<Span, SourceError> {
        self.files.span(file, start, end)
    }

    /// Returns a provenance record, or `None` for an ID outside this context.
    #[must_use]
    pub fn origin(&self, id: OriginId) -> Option<&Origin> {
        self.origins.get(id)
    }

    /// Returns the number of provenance records, including the unknown origin.
    #[must_use]
    pub fn origin_count(&self) -> usize {
        self.origins.len()
    }

    /// Appends a validated provenance record without interning it.
    ///
    /// All referenced spans and origins must already exist in this context. This
    /// append-only rule makes provenance cycles unrepresentable through the public
    /// API.
    ///
    /// # Errors
    ///
    /// Returns an error if a span or referenced origin is invalid, or if the origin
    /// ID space is exhausted.
    pub fn add_origin(&mut self, origin: Origin) -> Result<OriginId, OriginError> {
        self.validate_origin(&origin)?;
        self.origins
            .push(origin)
            .map_err(|EntityLimitError| OriginError::EntityLimit)
    }

    fn validate_origin(&self, origin: &Origin) -> Result<(), OriginError> {
        match origin {
            Origin::Unknown => Ok(()),
            Origin::Source(span) => self
                .files
                .validate_span(*span)
                .map_err(OriginError::InvalidSpan),
            Origin::CallSite { callee, caller } => {
                self.validate_origin_reference(*callee)?;
                self.validate_origin_reference(*caller)
            }
            Origin::Fused { inputs, .. } => {
                for input in inputs {
                    self.validate_origin_reference(*input)?;
                }
                Ok(())
            }
        }
    }

    fn validate_origin_reference(&self, id: OriginId) -> Result<(), OriginError> {
        self.origin(id)
            .map(|_| ())
            .ok_or(OriginError::InvalidReference { id })
    }
}

impl Default for SourceContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Failure while adding or resolving source text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceError {
    /// Source text is too large for the compiler's `u32` byte offsets.
    FileTooLarge {
        /// Actual source length in bytes.
        byte_len: usize,
    },
    /// No file with this ID exists in the source map.
    InvalidFile {
        /// Invalid file identity.
        file: FileId,
    },
    /// A span starts after it ends.
    ReversedSpan {
        /// Requested start offset.
        start: u32,
        /// Requested end offset.
        end: u32,
    },
    /// A span ends beyond its file.
    SpanOutOfBounds {
        /// File containing the attempted span.
        file: FileId,
        /// Requested exclusive end offset.
        end: u32,
        /// Actual source length in bytes.
        file_len: u32,
    },
    /// An offset splits a UTF-8 code point.
    NotCharBoundary {
        /// File containing the attempted span.
        file: FileId,
        /// Invalid byte offset.
        offset: u32,
    },
    /// The source file ID space is exhausted.
    EntityLimit,
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileTooLarge { byte_len } => {
                write!(
                    formatter,
                    "source file has {byte_len} bytes, exceeding u32 offsets"
                )
            }
            Self::InvalidFile { file } => write!(formatter, "invalid source file {file:?}"),
            Self::ReversedSpan { start, end } => {
                write!(
                    formatter,
                    "source span starts at {start} after ending at {end}"
                )
            }
            Self::SpanOutOfBounds {
                file,
                end,
                file_len,
            } => write!(
                formatter,
                "source span ends at {end}, beyond {file:?}'s {file_len} bytes"
            ),
            Self::NotCharBoundary { file, offset } => {
                write!(
                    formatter,
                    "offset {offset} splits a UTF-8 code point in {file:?}"
                )
            }
            Self::EntityLimit => formatter.write_str("source file ID space is exhausted"),
        }
    }
}

impl Error for SourceError {}

/// Failure while appending a provenance record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OriginError {
    /// A source span is invalid in this context.
    InvalidSpan(SourceError),
    /// A compound origin references an origin absent from this context.
    InvalidReference {
        /// Invalid origin identity.
        id: OriginId,
    },
    /// The provenance ID space is exhausted.
    EntityLimit,
}

impl fmt::Display for OriginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpan(error) => write!(formatter, "invalid source origin: {error}"),
            Self::InvalidReference { id } => {
                write!(formatter, "origin references missing provenance {id:?}")
            }
            Self::EntityLimit => formatter.write_str("provenance ID space is exhausted"),
        }
    }
}

impl Error for OriginError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSpan(error) => Some(error),
            Self::InvalidReference { .. } | Self::EntityLimit => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FileId, Origin, OriginError, OriginId, SourceContext, SourceError, SourceMap,
        SourcePosition,
    };

    #[test]
    fn files_are_dense_immutable_and_deterministic() {
        let mut sources = SourceMap::new();
        let first = sources.add_file("first.mdl", "one").unwrap();
        let second = sources.add_file("second.mdl", "two").unwrap();

        assert_eq!(first, FileId(0));
        assert_eq!(second, FileId(1));
        assert_eq!(sources.get(first).unwrap().name(), "first.mdl");
        assert_eq!(sources.get(first).unwrap().text(), "one");
        assert_eq!(sources.get(FileId(8)), None);
        assert_eq!(
            format!("{sources:?}"),
            "SourceMap { files: {FileId(0): SourceFile { name: \"first.mdl\", text: \"one\", line_starts: [0] }, FileId(1): SourceFile { name: \"second.mdl\", text: \"two\", line_starts: [0] }} }"
        );
    }

    #[test]
    fn spans_validate_order_bounds_and_utf8_boundaries() {
        let mut sources = SourceMap::new();
        let file = sources.add_file("unicode.mdl", "aéz").unwrap();

        let valid = sources.span(file, 1, 3).unwrap();
        assert_eq!(&sources.get(file).unwrap().text()[1..3], "é");
        assert_eq!(valid.len(), 2);
        assert_eq!(valid.file(), file);
        assert_eq!(valid.start(), 1);
        assert_eq!(valid.end(), 3);

        assert_eq!(
            sources.span(file, 3, 2),
            Err(SourceError::ReversedSpan { start: 3, end: 2 })
        );
        assert_eq!(
            sources.span(file, 0, 5),
            Err(SourceError::SpanOutOfBounds {
                file,
                end: 5,
                file_len: 4,
            })
        );
        assert_eq!(
            sources.span(file, 2, 3),
            Err(SourceError::NotCharBoundary { file, offset: 2 })
        );
        assert_eq!(
            sources.span(FileId(99), 0, 0),
            Err(SourceError::InvalidFile { file: FileId(99) })
        );
    }

    #[test]
    fn resolves_zero_based_lines_and_byte_columns() {
        let mut sources = SourceMap::new();
        let file = sources.add_file("lines.mdl", "aé\nxyz\n").unwrap();

        assert_eq!(
            sources.resolve_position(file, 0).unwrap(),
            SourcePosition {
                line: 0,
                byte_column: 0,
            }
        );
        assert_eq!(
            sources.resolve_position(file, 3).unwrap(),
            SourcePosition {
                line: 0,
                byte_column: 3,
            }
        );
        assert_eq!(
            sources.resolve_position(file, 4).unwrap(),
            SourcePosition {
                line: 1,
                byte_column: 0,
            }
        );
        assert_eq!(
            sources.resolve_position(file, 8).unwrap(),
            SourcePosition {
                line: 2,
                byte_column: 0,
            }
        );
        assert_eq!(
            sources.resolve_position(file, 2),
            Err(SourceError::NotCharBoundary { file, offset: 2 })
        );
    }

    #[test]
    fn context_always_starts_with_unknown_origin() {
        let context = SourceContext::new();
        assert_eq!(context.origin_count(), 1);
        assert_eq!(context.origin(OriginId::UNKNOWN), Some(&Origin::Unknown));
    }

    #[test]
    fn equal_origins_are_appended_without_interning() {
        let mut context = SourceContext::new();
        let file = context.add_file("same.mdl", "same").unwrap();
        let span = context.span(file, 0, 4).unwrap();

        let first = context.add_origin(Origin::Source(span)).unwrap();
        let second = context.add_origin(Origin::Source(span)).unwrap();

        assert_ne!(first, second);
        assert_eq!(context.origin(first), context.origin(second));
        assert_eq!(first, OriginId(1));
        assert_eq!(second, OriginId(2));
    }

    #[test]
    fn compound_origins_only_reference_existing_origins() {
        let mut context = SourceContext::new();
        let first = context.add_origin(Origin::Unknown).unwrap();
        let call_site = context
            .add_origin(Origin::CallSite {
                callee: first,
                caller: OriginId::UNKNOWN,
            })
            .unwrap();
        let fused = context
            .add_origin(Origin::Fused {
                inputs: vec![first, call_site],
                reason: Some("combined".into()),
            })
            .unwrap();

        assert_eq!(first, OriginId(1));
        assert_eq!(call_site, OriginId(2));
        assert_eq!(fused, OriginId(3));
        assert_eq!(context.origin_count(), 4);

        assert_eq!(
            context.add_origin(Origin::Fused {
                inputs: vec![OriginId(4)],
                reason: None,
            }),
            Err(OriginError::InvalidReference { id: OriginId(4) })
        );
        assert_eq!(context.origin_count(), 4);
    }

    #[test]
    fn invalid_origin_does_not_consume_an_id() {
        let mut context = SourceContext::new();
        let invalid = context.add_origin(Origin::CallSite {
            callee: OriginId(10),
            caller: OriginId::UNKNOWN,
        });
        assert_eq!(
            invalid,
            Err(OriginError::InvalidReference { id: OriginId(10) })
        );

        let next = context.add_origin(Origin::Unknown).unwrap();
        assert_eq!(next, OriginId(1));
    }
}
