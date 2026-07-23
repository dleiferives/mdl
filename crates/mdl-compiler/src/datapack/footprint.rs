use std::fmt::Write;

use crate::ir::minecraft::PackPath;

use super::{DatapackArtifact, TraceMap};

/// Semantic kind of one file in a generated datapack.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ArtifactFileKind {
    /// The root `pack.mcmeta` file.
    Metadata,
    /// One emitted `.mcfunction` file.
    Function,
    /// One emitted function-tag JSON file.
    FunctionTag,
    /// One emitted hidden advancement JSON file.
    Advancement,
}

/// Exact size of one emitted file in authoritative artifact order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactFileFootprint {
    path: PackPath,
    kind: ArtifactFileKind,
    utf8_bytes: usize,
}

impl ArtifactFileFootprint {
    /// Returns the normalized target-relative file path.
    #[must_use]
    pub const fn path(&self) -> &PackPath {
        &self.path
    }

    /// Returns the typed artifact role assigned during emission.
    #[must_use]
    pub const fn kind(&self) -> ArtifactFileKind {
        self.kind
    }

    /// Returns the exact number of bytes in this UTF-8 file.
    #[must_use]
    pub const fn utf8_bytes(&self) -> usize {
        self.utf8_bytes
    }
}

/// Exact, cached static footprint of one successfully emitted datapack.
///
/// These metrics describe generated code and metadata size. They do not estimate
/// how often any command executes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactFootprintReport {
    files: Box<[ArtifactFileFootprint]>,
    metadata_files: usize,
    function_files: usize,
    function_tag_files: usize,
    advancement_files: usize,
    physical_function_lines: usize,
    total_utf8_bytes: usize,
    maximum_function_line_utf16_units: usize,
    trace_records: usize,
}

impl ArtifactFootprintReport {
    pub(super) fn new(pack: &DatapackArtifact, trace: &TraceMap) -> Self {
        let mut metadata_files = 0usize;
        let mut function_files = 0usize;
        let mut function_tag_files = 0usize;
        let mut advancement_files = 0usize;
        let mut physical_function_lines = 0usize;
        let mut total_utf8_bytes = 0usize;
        let mut maximum_function_line_utf16_units = 0usize;
        let files = pack
            .files()
            .iter()
            .map(|file| {
                total_utf8_bytes = exact_sum(total_utf8_bytes, file.bytes().len());
                match file.kind() {
                    ArtifactFileKind::Metadata => metadata_files += 1,
                    ArtifactFileKind::Function => {
                        function_files += 1;
                        let contents = std::str::from_utf8(file.bytes())
                            .expect("successfully rendered function files are UTF-8");
                        for line in contents.split_terminator('\n') {
                            physical_function_lines = exact_sum(physical_function_lines, 1);
                            maximum_function_line_utf16_units =
                                maximum_function_line_utf16_units.max(line.encode_utf16().count());
                        }
                    }
                    ArtifactFileKind::FunctionTag => function_tag_files += 1,
                    ArtifactFileKind::Advancement => advancement_files += 1,
                }
                ArtifactFileFootprint {
                    path: file.path().clone(),
                    kind: file.kind(),
                    utf8_bytes: file.bytes().len(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            files,
            metadata_files,
            function_files,
            function_tag_files,
            advancement_files,
            physical_function_lines,
            total_utf8_bytes,
            maximum_function_line_utf16_units,
            trace_records: trace.records().count(),
        }
    }

    /// Returns exact per-file sizes in authoritative artifact order.
    #[must_use]
    pub fn files(&self) -> &[ArtifactFileFootprint] {
        &self.files
    }

    /// Returns the number of emitted metadata files.
    #[must_use]
    pub const fn metadata_files(&self) -> usize {
        self.metadata_files
    }

    /// Returns the number of emitted function files, including empty functions.
    #[must_use]
    pub const fn function_files(&self) -> usize {
        self.function_files
    }

    /// Returns the number of emitted function-tag files.
    #[must_use]
    pub const fn function_tag_files(&self) -> usize {
        self.function_tag_files
    }

    /// Returns the number of emitted advancement files.
    #[must_use]
    pub const fn advancement_files(&self) -> usize {
        self.advancement_files
    }

    /// Returns the number of physical command lines across function files.
    #[must_use]
    pub const fn physical_function_lines(&self) -> usize {
        self.physical_function_lines
    }

    /// Returns exact UTF-8 bytes across every artifact file.
    #[must_use]
    pub const fn total_utf8_bytes(&self) -> usize {
        self.total_utf8_bytes
    }

    /// Returns the longest physical function line in Java UTF-16 code units.
    #[must_use]
    pub const fn maximum_function_line_utf16_units(&self) -> usize {
        self.maximum_function_line_utf16_units
    }

    /// Returns the exact number of compiler-side physical-line trace records.
    #[must_use]
    pub const fn trace_records(&self) -> usize {
        self.trace_records
    }

    /// Renders all footprint fields in deterministic artifact order.
    #[must_use]
    pub fn dump(&self) -> String {
        let mut output = String::new();
        writeln!(
            output,
            "artifact-footprint files={} metadata={} functions={} function-tags={} advancements={} function-lines={} utf8-bytes={} max-function-line-utf16={} trace-records={}",
            self.files.len(),
            self.metadata_files,
            self.function_files,
            self.function_tag_files,
            self.advancement_files,
            self.physical_function_lines,
            self.total_utf8_bytes,
            self.maximum_function_line_utf16_units,
            self.trace_records
        )
        .unwrap();
        for file in &self.files {
            writeln!(
                output,
                "file {} kind={:?} utf8-bytes={}",
                file.path, file.kind, file.utf8_bytes
            )
            .unwrap();
        }
        output
    }
}

fn exact_sum(left: usize, right: usize) -> usize {
    left.checked_add(right)
        .expect("a complete in-memory artifact footprint fits usize")
}
