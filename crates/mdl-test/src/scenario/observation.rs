use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use quartz_nbt::NbtTag;

use super::names::{ResourceLocation, TestName};

const MAX_NBT_TEXT_BYTES: usize = 65_536;
const MAX_NBT_DEPTH: usize = 32;
const MAX_NBT_NODES: usize = 4_096;
const MAX_OBSERVATIONS: usize = 256;

/// Integer world block position.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlockPosition {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
    /// Z coordinate.
    pub z: i32,
}

impl BlockPosition {
    /// Creates one exact block position.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

impl fmt::Display for BlockPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {} {}", self.x, self.y, self.z)
    }
}

/// Canonical, exactly comparable NBT value.
///
/// Floating-point constructors reject non-finite values and normalize negative
/// zero. Compounds use ordered keys so rendering and diffs are deterministic.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NbtValue(NbtKind);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum NbtKind {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(u32),
    Double(u64),
    String(Box<str>),
    ByteArray(Box<[i8]>),
    List(Box<[NbtValue]>),
    Compound(BTreeMap<Box<str>, NbtValue>),
    IntArray(Box<[i32]>),
    LongArray(Box<[i64]>),
}

impl NbtValue {
    /// Creates an NBT byte.
    #[must_use]
    pub const fn byte(value: i8) -> Self {
        Self(NbtKind::Byte(value))
    }

    /// Creates an NBT short.
    #[must_use]
    pub const fn short(value: i16) -> Self {
        Self(NbtKind::Short(value))
    }

    /// Creates an NBT integer.
    #[must_use]
    pub const fn int(value: i32) -> Self {
        Self(NbtKind::Int(value))
    }

    /// Creates an NBT long.
    #[must_use]
    pub const fn long(value: i64) -> Self {
        Self(NbtKind::Long(value))
    }

    /// Creates a finite NBT float.
    ///
    /// # Errors
    ///
    /// Returns an error for NaN or infinity.
    pub fn float(value: f32) -> Result<Self, NonFiniteNumberError> {
        if !value.is_finite() {
            return Err(NonFiniteNumberError);
        }
        let normalized = if value == 0.0 { 0.0 } else { value };
        Ok(Self(NbtKind::Float(normalized.to_bits())))
    }

    /// Creates a finite NBT double.
    ///
    /// # Errors
    ///
    /// Returns an error for NaN or infinity.
    pub fn double(value: f64) -> Result<Self, NonFiniteNumberError> {
        if !value.is_finite() {
            return Err(NonFiniteNumberError);
        }
        let normalized = if value == 0.0 { 0.0 } else { value };
        Ok(Self(NbtKind::Double(normalized.to_bits())))
    }

    /// Creates an NBT string.
    #[must_use]
    pub fn string(value: impl Into<Box<str>>) -> Self {
        Self(NbtKind::String(value.into()))
    }

    /// Creates an NBT byte array.
    #[must_use]
    pub fn byte_array(value: impl Into<Box<[i8]>>) -> Self {
        Self(NbtKind::ByteArray(value.into()))
    }

    /// Creates an NBT list.
    #[must_use]
    pub fn list(value: impl Into<Box<[Self]>>) -> Self {
        Self(NbtKind::List(value.into()))
    }

    /// Creates an ordered NBT compound.
    #[must_use]
    pub fn compound(value: BTreeMap<Box<str>, Self>) -> Self {
        Self(NbtKind::Compound(value))
    }

    /// Creates an NBT integer array.
    #[must_use]
    pub fn int_array(value: impl Into<Box<[i32]>>) -> Self {
        Self(NbtKind::IntArray(value.into()))
    }

    /// Creates an NBT long array.
    #[must_use]
    pub fn long_array(value: impl Into<Box<[i64]>>) -> Self {
        Self(NbtKind::LongArray(value.into()))
    }

    /// Parses one arbitrary SNBT value under the scenario complexity limits.
    ///
    /// # Errors
    ///
    /// Returns an error for oversized text, invalid SNBT, non-finite numbers, or a
    /// value exceeding the bounded depth/node budget.
    pub fn parse_snbt(value: &str) -> Result<Self, NbtParseError> {
        if value.len() > MAX_NBT_TEXT_BYTES {
            return Err(NbtParseError(format!(
                "SNBT text has {} bytes, limit is {MAX_NBT_TEXT_BYTES}",
                value.len()
            )));
        }
        let wrapped = format!("{{__mdl_value:{value}}}");
        let mut compound =
            quartz_nbt::snbt::parse(&wrapped).map_err(|error| NbtParseError(error.to_string()))?;
        let tag = compound
            .inner_mut()
            .remove("__mdl_value")
            .ok_or_else(|| NbtParseError("SNBT parser omitted wrapped value".to_owned()))?;
        let parsed = Self::from_quartz(tag)?;
        let (depth, nodes) = parsed.complexity();
        if depth > MAX_NBT_DEPTH || nodes > MAX_NBT_NODES {
            return Err(NbtParseError(format!(
                "NBT value has depth {depth} and {nodes} nodes; limits are {MAX_NBT_DEPTH} and {MAX_NBT_NODES}"
            )));
        }
        Ok(parsed)
    }

    fn from_quartz(tag: NbtTag) -> Result<Self, NbtParseError> {
        Ok(match tag {
            NbtTag::Byte(value) => Self::byte(value),
            NbtTag::Short(value) => Self::short(value),
            NbtTag::Int(value) => Self::int(value),
            NbtTag::Long(value) => Self::long(value),
            NbtTag::Float(value) => {
                Self::float(value).map_err(|error| NbtParseError(error.to_string()))?
            }
            NbtTag::Double(value) => {
                Self::double(value).map_err(|error| NbtParseError(error.to_string()))?
            }
            NbtTag::ByteArray(values) => Self::byte_array(values),
            NbtTag::String(value) => Self::string(value),
            NbtTag::List(values) => Self::list(
                values
                    .into_inner()
                    .into_iter()
                    .map(Self::from_quartz)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            NbtTag::Compound(values) => Self::compound(
                values
                    .into_inner()
                    .into_iter()
                    .map(|(key, value)| Ok((key.into_boxed_str(), Self::from_quartz(value)?)))
                    .collect::<Result<BTreeMap<_, _>, NbtParseError>>()?,
            ),
            NbtTag::IntArray(values) => Self::int_array(values),
            NbtTag::LongArray(values) => Self::long_array(values),
        })
    }

    fn complexity(&self) -> (usize, usize) {
        match &self.0 {
            NbtKind::List(values) => aggregate_complexity(values.iter()),
            NbtKind::Compound(values) => aggregate_complexity(values.values()),
            NbtKind::Byte(_)
            | NbtKind::Short(_)
            | NbtKind::Int(_)
            | NbtKind::Long(_)
            | NbtKind::Float(_)
            | NbtKind::Double(_)
            | NbtKind::String(_)
            | NbtKind::ByteArray(_)
            | NbtKind::IntArray(_)
            | NbtKind::LongArray(_) => (1, 1),
        }
    }
}

fn aggregate_complexity<'a>(values: impl Iterator<Item = &'a NbtValue>) -> (usize, usize) {
    values.fold((1, 1), |(depth, nodes), value| {
        let (child_depth, child_nodes) = value.complexity();
        (
            depth.max(child_depth.saturating_add(1)),
            nodes.saturating_add(child_nodes),
        )
    })
}

/// Bounded SNBT parsing failure used by vanilla observation queries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NbtParseError(String);

impl fmt::Display for NbtParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for NbtParseError {}

impl fmt::Display for NbtValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            NbtKind::Byte(value) => write!(formatter, "{value}b"),
            NbtKind::Short(value) => write!(formatter, "{value}s"),
            NbtKind::Int(value) => write!(formatter, "{value}"),
            NbtKind::Long(value) => write!(formatter, "{value}L"),
            NbtKind::Float(bits) => write!(formatter, "{}f", f32::from_bits(*bits)),
            NbtKind::Double(bits) => write!(formatter, "{}d", f64::from_bits(*bits)),
            NbtKind::String(value) => write!(formatter, "{value:?}"),
            NbtKind::ByteArray(values) => display_array(formatter, "B", values, "b"),
            NbtKind::List(values) => display_sequence(formatter, "[", "]", values),
            NbtKind::Compound(values) => {
                formatter.write_str("{")?;
                for (index, (key, value)) in values.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(",")?;
                    }
                    write!(formatter, "{key:?}:{value}")?;
                }
                formatter.write_str("}")
            }
            NbtKind::IntArray(values) => display_array(formatter, "I", values, ""),
            NbtKind::LongArray(values) => display_array(formatter, "L", values, "L"),
        }
    }
}

/// A non-finite floating-point value cannot be normalized for comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NonFiniteNumberError;

impl fmt::Display for NonFiniteNumberError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("normalized observations require a finite number")
    }
}

impl Error for NonFiniteNumberError {}

/// Stable normalized block state and optional block-entity NBT.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NormalizedBlockState {
    block: ResourceLocation,
    properties: BTreeMap<Box<str>, Box<str>>,
    nbt: Option<NbtValue>,
}

impl NormalizedBlockState {
    /// Creates one normalized block observation.
    #[must_use]
    pub fn new(
        block: ResourceLocation,
        properties: BTreeMap<Box<str>, Box<str>>,
        nbt: Option<NbtValue>,
    ) -> Self {
        Self {
            block,
            properties,
            nbt,
        }
    }
}

/// Stable projected fields for one entity, excluding incidental UUID/time data.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EntityRecord(BTreeMap<Box<str>, NbtValue>);

impl EntityRecord {
    /// Creates a record from already selected stable fields.
    #[must_use]
    pub fn new(fields: BTreeMap<Box<str>, NbtValue>) -> Self {
        Self(fields)
    }

    /// Returns whether this record observes cardinality only.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Order-insensitive normalized entity observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityCollection {
    /// Every projected record must be unique.
    Set(BTreeSet<EntityRecord>),
    /// Duplicate projected records retain their exact multiplicity.
    Multiset(BTreeMap<EntityRecord, usize>),
}

impl EntityCollection {
    /// Returns whether every record intentionally projects no entity fields.
    #[must_use]
    pub fn is_cardinality_only(&self) -> bool {
        match self {
            Self::Set(records) => records.iter().all(EntityRecord::is_empty),
            Self::Multiset(records) => records.keys().all(EntityRecord::is_empty),
        }
    }
}

/// Declared equality mode for a possibly-many entity result.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntityComparison {
    /// Ignore selector order and require unique records.
    Set,
    /// Ignore selector order but preserve duplicate counts.
    Multiset,
}

/// Stable public server message effect.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NormalizedLogEffect {
    source: Box<str>,
    message: Box<str>,
}

impl NormalizedLogEffect {
    /// Creates a normalized public message.
    #[must_use]
    pub fn new(source: impl Into<Box<str>>, message: impl Into<Box<str>>) -> Self {
        Self {
            source: source.into(),
            message: message.into(),
        }
    }
}

/// Shape expected at an observation path.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ObservationKind {
    /// One optional scoreboard value.
    Score,
    /// One optional NBT value.
    Nbt,
    /// One block state.
    Block,
    /// A set or multiset of projected entities.
    Entities,
    /// Ordered public log effects.
    LogEffects,
}

/// Typed path into the declared world-state projection.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ObservationPath {
    /// One score holder in one test-owned objective.
    Score {
        /// Score holder or fake player.
        holder: Box<str>,
        /// Objective declared by the scenario.
        objective: TestName,
    },
    /// One path in test-owned command storage.
    Storage {
        /// Storage resource.
        storage: ResourceLocation,
        /// Stable NBT path text interpreted by the vanilla query layer.
        path: Box<str>,
    },
    /// One exact block in an explicit dimension.
    Block {
        /// Dimension resource.
        dimension: ResourceLocation,
        /// World position.
        position: BlockPosition,
    },
    /// All controlled entities carrying one test-owned tag.
    Entities {
        /// Ownership/isolation tag.
        tag: TestName,
        /// Comparison semantics.
        comparison: EntityComparison,
    },
    /// Ordered public effects attributed to a stable marker.
    LogEffects {
        /// Unique marker embedded by the harness wrapper.
        marker: TestName,
    },
}

impl ObservationPath {
    /// Returns the required observation shape.
    #[must_use]
    pub const fn kind(&self) -> ObservationKind {
        match self {
            Self::Score { .. } => ObservationKind::Score,
            Self::Storage { .. } => ObservationKind::Nbt,
            Self::Block { .. } => ObservationKind::Block,
            Self::Entities { .. } => ObservationKind::Entities,
            Self::LogEffects { .. } => ObservationKind::LogEffects,
        }
    }

    /// Rejects newlines and empty unparsed command arguments.
    ///
    /// # Errors
    ///
    /// Returns an error when a score holder or storage NBT path is unsafe.
    pub fn validate(&self) -> Result<(), ObservationSetError> {
        let text = match self {
            Self::Score { holder, .. } => Some(("score holder", holder.as_ref())),
            Self::Storage { path, .. } => Some(("NBT path", path.as_ref())),
            _ => None,
        };
        if let Some((kind, text)) = text
            && (text.is_empty() || text.contains(['\n', '\r']))
        {
            return Err(ObservationSetError::UnsafePathText {
                kind,
                value: text.to_owned(),
            });
        }
        Ok(())
    }
}

impl fmt::Display for ObservationPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Score { holder, objective } => write!(formatter, "score/{objective}/{holder}"),
            Self::Storage { storage, path } => write!(formatter, "storage/{storage}/{path}"),
            Self::Block {
                dimension,
                position,
            } => write!(formatter, "block/{dimension}/{position}"),
            Self::Entities { tag, comparison } => {
                write!(formatter, "entities/{tag}/{comparison:?}")
            }
            Self::LogEffects { marker } => write!(formatter, "log/{marker}"),
        }
    }
}

/// One normalized value in a scenario projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservationValue {
    /// Missing scores remain distinct from zero.
    Score(Option<i32>),
    /// Missing storage paths remain distinct from empty compounds/lists.
    Nbt(Option<NbtValue>),
    /// Exact normalized block state.
    Block(NormalizedBlockState),
    /// Order-insensitive entity records.
    Entities(EntityCollection),
    /// Ordered public messages.
    LogEffects(Box<[NormalizedLogEffect]>),
}

impl ObservationValue {
    /// Returns this value's normalized shape.
    #[must_use]
    pub const fn kind(&self) -> ObservationKind {
        match self {
            Self::Score(_) => ObservationKind::Score,
            Self::Nbt(_) => ObservationKind::Nbt,
            Self::Block(_) => ObservationKind::Block,
            Self::Entities(_) => ObservationKind::Entities,
            Self::LogEffects(_) => ObservationKind::LogEffects,
        }
    }

    fn validate_complexity(&self) -> Result<(), ObservationSetError> {
        let mut check_nbt = |value: &NbtValue| {
            let (depth, nodes) = value.complexity();
            if depth > MAX_NBT_DEPTH || nodes > MAX_NBT_NODES {
                Err(ObservationSetError::NbtTooComplex { depth, nodes })
            } else {
                Ok(())
            }
        };
        match self {
            Self::Nbt(Some(value)) => check_nbt(value),
            Self::Block(value) => value.nbt.as_ref().map_or(Ok(()), &mut check_nbt),
            Self::Entities(collection) => {
                let records: Box<dyn Iterator<Item = &EntityRecord> + '_> = match collection {
                    EntityCollection::Set(records) => Box::new(records.iter()),
                    EntityCollection::Multiset(records) => Box::new(records.keys()),
                };
                for record in records {
                    for value in record.0.values() {
                        check_nbt(value)?;
                    }
                }
                Ok(())
            }
            Self::Score(_) | Self::Nbt(None) | Self::LogEffects(_) => Ok(()),
        }
    }
}

impl fmt::Display for ObservationValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Score(Some(value)) => write!(formatter, "score({value})"),
            Self::Score(None) => formatter.write_str("score(missing)"),
            Self::Nbt(Some(value)) => write!(formatter, "nbt({value})"),
            Self::Nbt(None) => formatter.write_str("nbt(missing)"),
            Self::Block(value) => write!(formatter, "block({value:?})"),
            Self::Entities(value) => write!(formatter, "entities({value:?})"),
            Self::LogEffects(value) => write!(formatter, "logs({value:?})"),
        }
    }
}

/// Duplicate-free, deterministically ordered world-state projection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObservationSet(BTreeMap<ObservationPath, ObservationValue>);

impl ObservationSet {
    /// Validates and collects observations without overwriting duplicates.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe/duplicate paths or a path/value shape mismatch.
    pub fn try_new(
        observations: impl IntoIterator<Item = (ObservationPath, ObservationValue)>,
    ) -> Result<Self, ObservationSetError> {
        let mut values = BTreeMap::new();
        for (path, value) in observations {
            if values.len() == MAX_OBSERVATIONS {
                return Err(ObservationSetError::TooManyObservations {
                    limit: MAX_OBSERVATIONS,
                });
            }
            path.validate()?;
            value.validate_complexity()?;
            let expected_kind = path.kind();
            if expected_kind != value.kind() {
                return Err(ObservationSetError::KindMismatch {
                    path,
                    expected: expected_kind,
                    actual: value.kind(),
                });
            }
            if values.insert(path.clone(), value).is_some() {
                return Err(ObservationSetError::DuplicatePath(path));
            }
        }
        Ok(Self(values))
    }

    /// Returns an empty projection.
    #[must_use]
    pub const fn empty() -> Self {
        Self(BTreeMap::new())
    }

    /// Iterates observations in stable path order.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&ObservationPath, &ObservationValue)> {
        self.0.iter()
    }

    /// Returns the value at one declared path.
    #[must_use]
    pub fn get(&self, path: &ObservationPath) -> Option<&ObservationValue> {
        self.0.get(path)
    }

    /// Returns whether the projection is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Invalid normalized observation declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservationSetError {
    /// One projection exceeds the fixed harness query budget.
    TooManyObservations {
        /// Maximum supported path count.
        limit: usize,
    },
    /// One NBT tree exceeds bounded comparison resources.
    NbtTooComplex {
        /// Observed maximum depth.
        depth: usize,
        /// Observed total node count.
        nodes: usize,
    },
    /// The same path was declared twice.
    DuplicatePath(ObservationPath),
    /// A normalized value has the wrong shape for its path.
    KindMismatch {
        /// Invalid path.
        path: ObservationPath,
        /// Required shape.
        expected: ObservationKind,
        /// Supplied shape.
        actual: ObservationKind,
    },
    /// Unparsed command argument text could inject another line or is empty.
    UnsafePathText {
        /// Argument kind.
        kind: &'static str,
        /// Rejected text.
        value: String,
    },
}

impl fmt::Display for ObservationSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyObservations { limit } => {
                write!(formatter, "observation set exceeds the {limit}-path limit")
            }
            Self::NbtTooComplex { depth, nodes } => write!(
                formatter,
                "NBT observation has depth {depth} and {nodes} nodes; limits are {MAX_NBT_DEPTH} and {MAX_NBT_NODES}"
            ),
            Self::DuplicatePath(path) => write!(formatter, "duplicate observation path {path}"),
            Self::KindMismatch {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "observation {path} requires {expected:?}, received {actual:?}"
            ),
            Self::UnsafePathText { kind, value } => {
                write!(formatter, "unsafe {kind} in observation path: {value:?}")
            }
        }
    }
}

impl Error for ObservationSetError {}

/// One concise path-specific mismatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationDifference {
    path: String,
    detail: String,
}

impl ObservationDifference {
    /// Returns the observation or nested NBT path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the expected/actual summary.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for ObservationDifference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.detail)
    }
}

/// Bounded comparison result suitable for a test failure report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationComparison {
    differences: Box<[ObservationDifference]>,
    omitted: usize,
}

impl ObservationComparison {
    /// Returns whether projections matched exactly.
    #[must_use]
    pub fn is_equal(&self) -> bool {
        self.differences.is_empty() && self.omitted == 0
    }

    /// Returns reported differences, bounded by the caller's limit.
    #[must_use]
    pub fn differences(&self) -> &[ObservationDifference] {
        &self.differences
    }

    /// Returns the number of additional differences suppressed.
    #[must_use]
    pub const fn omitted(&self) -> usize {
        self.omitted
    }
}

impl fmt::Display for ObservationComparison {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_equal() {
            return formatter.write_str("observations match");
        }
        for (index, difference) in self.differences.iter().enumerate() {
            if index != 0 {
                formatter.write_str("\n")?;
            }
            write!(formatter, "{difference}")?;
        }
        if self.omitted != 0 {
            write!(formatter, "\n… {} more differences omitted", self.omitted)?;
        }
        Ok(())
    }
}

/// Compares two normalized projections and reports at most `max_differences` items.
#[must_use]
pub fn compare_observations(
    expected: &ObservationSet,
    actual: &ObservationSet,
    max_differences: usize,
) -> ObservationComparison {
    let paths = expected
        .0
        .keys()
        .chain(actual.0.keys())
        .collect::<BTreeSet<_>>();
    let mut collector = DifferenceCollector::new(max_differences);
    for path in paths {
        match (expected.0.get(path), actual.0.get(path)) {
            (Some(expected), Some(actual)) if expected != actual => {
                compare_value(&path.to_string(), expected, actual, &mut collector);
            }
            (Some(expected), None) => collector.push(
                path.to_string(),
                format!("missing; expected {}", summarize(expected)),
            ),
            (None, Some(actual)) => collector.push(
                path.to_string(),
                format!("unexpected {}", summarize(actual)),
            ),
            _ => {}
        }
    }
    collector.finish()
}

struct DifferenceCollector {
    limit: usize,
    differences: Vec<ObservationDifference>,
    omitted: usize,
}

impl DifferenceCollector {
    const fn new(limit: usize) -> Self {
        Self {
            limit,
            differences: vec![],
            omitted: 0,
        }
    }

    fn push(&mut self, path: String, detail: String) {
        if self.differences.len() < self.limit {
            self.differences
                .push(ObservationDifference { path, detail });
        } else {
            self.omitted += 1;
        }
    }

    fn finish(self) -> ObservationComparison {
        ObservationComparison {
            differences: self.differences.into_boxed_slice(),
            omitted: self.omitted,
        }
    }
}

fn compare_value(
    path: &str,
    expected: &ObservationValue,
    actual: &ObservationValue,
    collector: &mut DifferenceCollector,
) {
    match (expected, actual) {
        (ObservationValue::Nbt(Some(expected)), ObservationValue::Nbt(Some(actual))) => {
            compare_nbt(path, expected, actual, collector);
        }
        _ => collector.push(
            path.to_owned(),
            format!(
                "expected {}, actual {}",
                summarize(expected),
                summarize(actual)
            ),
        ),
    }
}

fn compare_nbt(
    path: &str,
    expected: &NbtValue,
    actual: &NbtValue,
    collector: &mut DifferenceCollector,
) {
    match (&expected.0, &actual.0) {
        (NbtKind::Compound(expected), NbtKind::Compound(actual)) => {
            let keys = expected
                .keys()
                .chain(actual.keys())
                .collect::<BTreeSet<_>>();
            for key in keys {
                let nested = format!("{path}.{key}");
                compare_optional_nbt(&nested, expected.get(key), actual.get(key), collector);
            }
        }
        (NbtKind::List(expected), NbtKind::List(actual)) => {
            let length = expected.len().max(actual.len());
            for index in 0..length {
                let nested = format!("{path}[{index}]");
                compare_optional_nbt(&nested, expected.get(index), actual.get(index), collector);
            }
        }
        _ => collector.push(
            path.to_owned(),
            format!("expected {}, actual {}", clipped(expected), clipped(actual)),
        ),
    }
}

fn compare_optional_nbt(
    path: &str,
    expected: Option<&NbtValue>,
    actual: Option<&NbtValue>,
    collector: &mut DifferenceCollector,
) {
    match (expected, actual) {
        (Some(expected), Some(actual)) if expected != actual => {
            compare_nbt(path, expected, actual, collector);
        }
        (Some(expected), None) => {
            collector.push(
                path.to_owned(),
                format!("missing; expected {}", clipped(expected)),
            );
        }
        (None, Some(actual)) => {
            collector.push(path.to_owned(), format!("unexpected {}", clipped(actual)));
        }
        _ => {}
    }
}

fn summarize(value: &ObservationValue) -> String {
    clip_text(&value.to_string())
}

fn clipped(value: &NbtValue) -> String {
    clip_text(&value.to_string())
}

fn clip_text(value: &str) -> String {
    const MAX_CHARS: usize = 160;
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn display_sequence<T: fmt::Display>(
    formatter: &mut fmt::Formatter<'_>,
    open: &str,
    close: &str,
    values: &[T],
) -> fmt::Result {
    formatter.write_str(open)?;
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            formatter.write_str(",")?;
        }
        write!(formatter, "{value}")?;
    }
    formatter.write_str(close)
}

fn display_array<T: fmt::Display>(
    formatter: &mut fmt::Formatter<'_>,
    kind: &str,
    values: &[T],
    suffix: &str,
) -> fmt::Result {
    write!(formatter, "[{kind};")?;
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            formatter.write_str(",")?;
        }
        write!(formatter, "{value}{suffix}")?;
    }
    formatter.write_str("]")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        MAX_NBT_DEPTH, MAX_NBT_TEXT_BYTES, MAX_OBSERVATIONS, NbtValue, ObservationPath,
        ObservationSet, ObservationValue, compare_observations,
    };
    use crate::scenario::{ResourceLocation, TestName};

    fn storage_path() -> ObservationPath {
        ObservationPath::Storage {
            storage: ResourceLocation::parse("mdl_test:result/example").unwrap(),
            path: "value".into(),
        }
    }

    #[test]
    fn observations_reject_duplicates_shape_mismatches_and_newlines() {
        let path = storage_path();
        assert!(
            ObservationSet::try_new([
                (path.clone(), ObservationValue::Nbt(None)),
                (path.clone(), ObservationValue::Nbt(None)),
            ])
            .unwrap_err()
            .to_string()
            .contains("duplicate")
        );
        assert!(
            ObservationSet::try_new([(path, ObservationValue::Score(Some(1)))])
                .unwrap_err()
                .to_string()
                .contains("requires Nbt")
        );
        assert!(
            ObservationSet::try_new([(
                ObservationPath::Score {
                    holder: "#ok\ninjected".into(),
                    objective: TestName::new("mdl.test").unwrap(),
                },
                ObservationValue::Score(Some(1)),
            )])
            .unwrap_err()
            .to_string()
            .contains("unsafe")
        );
    }

    #[test]
    fn nested_nbt_diff_is_path_specific_and_bounded() {
        let compound = |left, right| {
            NbtValue::compound(BTreeMap::from([
                (Box::<str>::from("left"), NbtValue::int(left)),
                (
                    Box::<str>::from("nested"),
                    NbtValue::list(vec![NbtValue::int(right)].into_boxed_slice()),
                ),
            ]))
        };
        let expected = ObservationSet::try_new([(
            storage_path(),
            ObservationValue::Nbt(Some(compound(1, 2))),
        )])
        .unwrap();
        let actual = ObservationSet::try_new([(
            storage_path(),
            ObservationValue::Nbt(Some(compound(3, 4))),
        )])
        .unwrap();

        let comparison = compare_observations(&expected, &actual, 1);
        assert!(!comparison.is_equal());
        assert_eq!(comparison.differences().len(), 1);
        assert_eq!(comparison.omitted(), 1);
        assert!(comparison.to_string().contains(".left"));
        assert!(comparison.to_string().contains("1 more"));
    }

    #[test]
    fn negative_zero_normalizes_and_non_finite_values_fail() {
        assert_eq!(
            NbtValue::double(-0.0).unwrap(),
            NbtValue::double(0.0).unwrap()
        );
        assert!(NbtValue::float(f32::NAN).is_err());
        assert!(NbtValue::double(f64::INFINITY).is_err());
    }

    #[test]
    fn snbt_parsing_is_canonical_and_resource_bounded() {
        let parsed =
            NbtValue::parse_snbt(r#"{z:[I;1,-2],a:{text:"snowman ☃",bytes:[B;1b,-2b]},long:7L}"#)
                .unwrap();
        assert_eq!(
            parsed.to_string(),
            r#"{"a":{"bytes":[B;1b,-2b],"text":"snowman ☃"},"long":7L,"z":[I;1,-2]}"#
        );
        assert!(NbtValue::parse_snbt(&"x".repeat(MAX_NBT_TEXT_BYTES + 1)).is_err());

        let mut deep = NbtValue::int(0);
        for _ in 0..MAX_NBT_DEPTH {
            deep = NbtValue::list(vec![deep]);
        }
        assert!(
            ObservationSet::try_new([(storage_path(), ObservationValue::Nbt(Some(deep)))])
                .unwrap_err()
                .to_string()
                .contains("depth")
        );
    }

    #[test]
    fn observation_count_is_bounded_before_server_startup() {
        let objective = TestName::new("mdl.bound").unwrap();
        let observations = (0..=MAX_OBSERVATIONS).map(|index| {
            (
                ObservationPath::Score {
                    holder: format!("#v{index}").into_boxed_str(),
                    objective: objective.clone(),
                },
                ObservationValue::Score(Some(i32::try_from(index).unwrap())),
            )
        });
        assert!(
            ObservationSet::try_new(observations)
                .unwrap_err()
                .to_string()
                .contains("path limit")
        );
    }
}
