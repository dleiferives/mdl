//! Checked deterministic datapack emission.

mod emit;
mod footprint;

use std::num::NonZeroU64;

use crate::entity::{EntityId, EntityVec};
use crate::ir::minecraft::{CommandId, FunctionResourceId, McFunctionId, PackPath};
use crate::source::OriginId;
use crate::target::JavaEditionTarget;

pub use emit::{EmissionOptions, emit_datapack};
pub use footprint::{ArtifactFileFootprint, ArtifactFileKind, ArtifactFootprintReport};

/// One target-relative file in deterministic artifact order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackFile {
    path: PackPath,
    bytes: Vec<u8>,
    kind: ArtifactFileKind,
}

impl PackFile {
    pub(crate) const fn new(path: PackPath, bytes: Vec<u8>, kind: ArtifactFileKind) -> Self {
        Self { path, bytes, kind }
    }

    /// Returns the normalized logical artifact path.
    #[must_use]
    pub const fn path(&self) -> &PackPath {
        &self.path
    }

    /// Returns exact file bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) const fn kind(&self) -> ArtifactFileKind {
        self.kind
    }
}

/// A complete deterministic in-memory datapack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatapackArtifact {
    files: Vec<PackFile>,
}

impl DatapackArtifact {
    pub(crate) const fn new(files: Vec<PackFile>) -> Self {
        Self { files }
    }

    /// Returns files sorted by normalized logical path.
    #[must_use]
    pub fn files(&self) -> &[PackFile] {
        &self.files
    }

    /// Looks up one exact logical artifact path.
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&PackFile> {
        self.files.iter().find(|file| file.path().as_str() == path)
    }
}

/// Compiler-side source correspondence for emitted function lines.
#[derive(Clone, Debug)]
pub struct TraceMap {
    target: JavaEditionTarget,
    functions: EntityVec<McFunctionId, FunctionTrace>,
}

impl TraceMap {
    pub(crate) const fn new(
        target: JavaEditionTarget,
        functions: EntityVec<McFunctionId, FunctionTrace>,
    ) -> Self {
        Self { target, functions }
    }

    /// Returns the target whose physical lines are described.
    #[must_use]
    pub const fn target(&self) -> JavaEditionTarget {
        self.target
    }

    /// Returns the exact data-pack format derived from the target.
    #[must_use]
    pub fn pack_format(&self) -> [u32; 2] {
        self.target.spec().data_pack_format()
    }

    /// Looks up one function trace by the corresponding program ID.
    #[must_use]
    pub fn function(&self, function: McFunctionId) -> Option<&FunctionTrace> {
        self.functions.get(function)
    }

    /// Iterates flat physical-line records in function/command order.
    pub fn records(&self) -> impl Iterator<Item = TraceRecord<'_>> + '_ {
        self.functions.iter().flat_map(|(function_id, function)| {
            function
                .origins
                .iter()
                .map(move |(command, origin)| TraceRecord {
                    function_id,
                    function: &function.function,
                    command,
                    origin: *origin,
                })
        })
    }
}

/// Dense origins for one emitted function.
#[derive(Clone, Debug)]
pub struct FunctionTrace {
    function: FunctionResourceId,
    origins: EntityVec<CommandId, OriginId>,
}

impl FunctionTrace {
    pub(crate) const fn new(
        function: FunctionResourceId,
        origins: EntityVec<CommandId, OriginId>,
    ) -> Self {
        Self { function, origins }
    }

    /// Returns the function resource.
    #[must_use]
    pub const fn function(&self) -> &FunctionResourceId {
        &self.function
    }

    /// Returns the origin for one physical command line.
    #[must_use]
    pub fn origin(&self, command: CommandId) -> Option<OriginId> {
        self.origins.get(command).copied()
    }

    /// Returns the number of physical command lines.
    #[must_use]
    pub fn len(&self) -> usize {
        self.origins.len()
    }

    /// Returns whether the function emitted zero physical lines.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }
}

/// A borrowed flat trace view; only irreducible identity/provenance is stored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceRecord<'a> {
    function_id: McFunctionId,
    function: &'a FunctionResourceId,
    command: CommandId,
    origin: OriginId,
}

impl<'a> TraceRecord<'a> {
    /// Returns the corresponding program function ID.
    #[must_use]
    pub const fn function_id(self) -> McFunctionId {
        self.function_id
    }

    /// Returns the serialized function resource.
    #[must_use]
    pub const fn function(self) -> &'a FunctionResourceId {
        self.function
    }

    /// Returns the function-local command identity.
    #[must_use]
    pub const fn command(self) -> CommandId {
        self.command
    }

    /// Returns line-level compiler provenance.
    #[must_use]
    pub const fn origin(self) -> OriginId {
        self.origin
    }

    /// Returns the one-based physical line derived from dense command order.
    #[must_use]
    pub fn line(self) -> NonZeroU64 {
        let line = u64::from(self.command.index()) + 1;
        NonZeroU64::new(line).unwrap_or(NonZeroU64::MIN)
    }
}

/// Complete checked emission output.
#[derive(Clone, Debug)]
pub struct EmissionOutput {
    pack: DatapackArtifact,
    trace: TraceMap,
    footprint: ArtifactFootprintReport,
}

impl EmissionOutput {
    pub(crate) fn new(pack: DatapackArtifact, trace: TraceMap) -> Self {
        let footprint = ArtifactFootprintReport::new(&pack, &trace);
        Self {
            pack,
            trace,
            footprint,
        }
    }

    /// Returns the deterministic in-memory datapack.
    #[must_use]
    pub const fn pack(&self) -> &DatapackArtifact {
        &self.pack
    }

    /// Returns compiler-side physical-line traceability.
    #[must_use]
    pub const fn trace(&self) -> &TraceMap {
        &self.trace
    }

    /// Returns exact cached metrics for the completed emitted artifact.
    #[must_use]
    pub const fn footprint(&self) -> &ArtifactFootprintReport {
        &self.footprint
    }

    /// Consumes the output into its independently owned artifact, trace, and report.
    #[must_use]
    pub fn into_parts(self) -> (DatapackArtifact, TraceMap, ArtifactFootprintReport) {
        (self.pack, self.trace, self.footprint)
    }
}
