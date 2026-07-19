use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use super::names::{GenerationNonce, NameError, ResourceLocation, ScenarioId, TestName};
use super::observation::{
    BlockPosition, EntityCollection, EntityComparison, ObservationPath, ObservationSet,
    ObservationSetError, ObservationValue,
};

/// Core optimization selection represented in a scenario matrix.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CorePolicy {
    /// Preserve the verified Core program without optional optimization.
    None,
    /// Apply the baseline Core pipeline.
    Baseline,
}

/// Minecraft optimization selection represented in a scenario matrix.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MinecraftPolicy {
    /// Preserve unoptimized Minecraft IR.
    None,
    /// Apply the baseline Minecraft pipeline.
    Baseline,
}

/// One product of independently selectable Core and Minecraft policies.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompilerPolicy {
    core: CorePolicy,
    minecraft: MinecraftPolicy,
}

impl CompilerPolicy {
    /// Every supported policy product in deterministic order.
    pub const ALL: [Self; 4] = [
        Self::new(CorePolicy::None, MinecraftPolicy::None),
        Self::new(CorePolicy::None, MinecraftPolicy::Baseline),
        Self::new(CorePolicy::Baseline, MinecraftPolicy::None),
        Self::new(CorePolicy::Baseline, MinecraftPolicy::Baseline),
    ];

    /// Creates one policy product.
    #[must_use]
    pub const fn new(core: CorePolicy, minecraft: MinecraftPolicy) -> Self {
        Self { core, minecraft }
    }

    /// Returns the Core policy.
    #[must_use]
    pub const fn core(self) -> CorePolicy {
        self.core
    }

    /// Returns the Minecraft policy.
    #[must_use]
    pub const fn minecraft(self) -> MinecraftPolicy {
        self.minecraft
    }
}

impl fmt::Display for CompilerPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "core={:?},minecraft={:?}",
            self.core, self.minecraft
        )
    }
}

/// One safe function invocation understood by the first scenario runner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScenarioEntry {
    /// Invoke one logical source export; each policy deployment resolves its ABI.
    Export(TestName),
}

/// Explicit executor for a scenario invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Executor {
    /// Dedicated-server console command source.
    Server,
    /// Exactly the contexts selected by an explicit entity selector.
    Entities(EntitySelector),
}

/// Validated single-line Minecraft entity selector used only at the server boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntitySelector(Box<str>);

impl EntitySelector {
    /// Validates a nonempty selector beginning with `@`.
    ///
    /// # Errors
    ///
    /// Returns an error for newlines, missing selector prefix, or empty text.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, ScenarioValidationError> {
        let value = value.into();
        if value.starts_with('@') && !value.contains(['\n', '\r']) {
            Ok(Self(value))
        } else {
            Err(ScenarioValidationError::UnsafeSelector(value.into()))
        }
    }

    /// Returns selector text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One coordinate relative to world, current position, or local facing frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Coordinate {
    /// Absolute world coordinate.
    Absolute(f64),
    /// Offset from the current execution position (`~`).
    Relative(f64),
    /// Offset in the current local facing frame (`^`).
    Local(f64),
}

impl Coordinate {
    const fn value(self) -> f64 {
        match self {
            Self::Absolute(value) | Self::Relative(value) | Self::Local(value) => value,
        }
    }

    const fn is_local(self) -> bool {
        matches!(self, Self::Local(_))
    }
}

/// Validated three-dimensional execution position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position([Coordinate; 3]);

impl Position {
    /// Creates a finite coordinate triple without mixing local and non-local axes.
    ///
    /// # Errors
    ///
    /// Returns an error for NaN/infinity or a mixed `^` coordinate frame.
    pub fn new(
        x: Coordinate,
        y: Coordinate,
        z: Coordinate,
    ) -> Result<Self, ScenarioValidationError> {
        let coordinates = [x, y, z];
        validate_coordinates(coordinates)?;
        Ok(Self(coordinates))
    }

    /// Returns x/y/z coordinates.
    #[must_use]
    pub const fn coordinates(self) -> [Coordinate; 3] {
        self.0
    }
}

/// Absolute or current-frame-relative yaw and pitch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation {
    yaw: Coordinate,
    pitch: Coordinate,
}

impl Rotation {
    /// Creates finite non-local rotation coordinates.
    ///
    /// # Errors
    ///
    /// Returns an error for NaN/infinity or local (`^`) coordinates.
    pub fn new(yaw: Coordinate, pitch: Coordinate) -> Result<Self, ScenarioValidationError> {
        if !yaw.value().is_finite() || !pitch.value().is_finite() {
            return Err(ScenarioValidationError::NonFiniteCoordinate);
        }
        if yaw.is_local() || pitch.is_local() {
            return Err(ScenarioValidationError::LocalRotation);
        }
        Ok(Self { yaw, pitch })
    }

    /// Returns yaw and pitch coordinates.
    #[must_use]
    pub const fn coordinates(self) -> [Coordinate; 2] {
        [self.yaw, self.pitch]
    }
}

/// Execution anchor used by local coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Anchor {
    /// Entity feet.
    Feet,
    /// Entity eyes.
    Eyes,
}

/// Complete execution-frame projection relevant to a scenario.
#[derive(Clone, Debug, PartialEq)]
pub struct InvocationFrame {
    executor: Executor,
    position: Option<Position>,
    rotation: Option<Rotation>,
    dimension: Option<ResourceLocation>,
    anchor: Option<Anchor>,
}

impl InvocationFrame {
    /// Creates the ordinary server-console frame.
    #[must_use]
    pub const fn server() -> Self {
        Self {
            executor: Executor::Server,
            position: None,
            rotation: None,
            dimension: None,
            anchor: None,
        }
    }

    /// Replaces the executor.
    #[must_use]
    pub fn with_executor(mut self, executor: Executor) -> Self {
        self.executor = executor;
        self
    }

    /// Sets an explicit execution position.
    #[must_use]
    pub const fn with_position(mut self, position: Position) -> Self {
        self.position = Some(position);
        self
    }

    /// Sets an explicit execution rotation.
    #[must_use]
    pub const fn with_rotation(mut self, rotation: Rotation) -> Self {
        self.rotation = Some(rotation);
        self
    }

    /// Sets an explicit execution dimension.
    #[must_use]
    pub fn with_dimension(mut self, dimension: ResourceLocation) -> Self {
        self.dimension = Some(dimension);
        self
    }

    /// Sets an explicit execution anchor.
    #[must_use]
    pub const fn with_anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = Some(anchor);
        self
    }

    /// Returns the executor.
    #[must_use]
    pub const fn executor(&self) -> &Executor {
        &self.executor
    }

    /// Returns the optional explicit position.
    #[must_use]
    pub const fn position(&self) -> Option<Position> {
        self.position
    }

    /// Returns the optional explicit rotation.
    #[must_use]
    pub const fn rotation(&self) -> Option<Rotation> {
        self.rotation
    }

    /// Returns the optional explicit dimension.
    #[must_use]
    pub const fn dimension(&self) -> Option<&ResourceLocation> {
        self.dimension.as_ref()
    }

    /// Returns the optional explicit anchor.
    #[must_use]
    pub const fn anchor(&self) -> Option<Anchor> {
        self.anchor
    }
}

impl Default for InvocationFrame {
    fn default() -> Self {
        Self::server()
    }
}

/// Independently observed command result channels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandOutcomeExpectation {
    success: OutcomeChannelExpectation<bool>,
    result: OutcomeChannelExpectation<i32>,
    continuation: CompletionExpectation,
}

impl CommandOutcomeExpectation {
    /// Creates an exact or partial three-channel expectation.
    #[must_use]
    pub const fn new(
        success: OutcomeChannelExpectation<bool>,
        result: OutcomeChannelExpectation<i32>,
        continuation: CompletionExpectation,
    ) -> Self {
        Self {
            success,
            result,
            continuation,
        }
    }

    /// Returns the command-success channel contract.
    #[must_use]
    pub const fn success(self) -> OutcomeChannelExpectation<bool> {
        self.success
    }

    /// Returns the command-result channel contract.
    #[must_use]
    pub const fn result(self) -> OutcomeChannelExpectation<i32> {
        self.result
    }

    /// Returns the independent outer-continuation contract.
    #[must_use]
    pub const fn continuation(self) -> CompletionExpectation {
        self.continuation
    }
}

/// Expected availability and value of one command outcome channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutcomeChannelExpectation<T> {
    /// Do not assert this channel.
    Unobserved,
    /// The nested command must not publish this channel.
    Unavailable,
    /// The nested command must publish this exact value.
    Value(T),
}

/// Expected completion of the harness continuation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionExpectation {
    /// The outer wrapper must continue synchronously.
    Continued,
    /// The command sequence must terminate before the outer marker.
    Interrupted,
    /// Do not assert the outer continuation channel.
    Unobserved,
}

/// Minecraft hard limit being calibrated without semantic instrumentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExactLimitKind {
    /// Maximum command sequence length.
    Sequence,
    /// Maximum command context forks.
    Fork,
}

/// Bare-root hard-limit contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactLimitContract {
    kind: ExactLimitKind,
    configured_limit: u32,
    completion: ObservationPath,
    root_completes: bool,
}

impl ExactLimitContract {
    /// Creates one exact boundary probe.
    ///
    /// The measured root owns the completion write: it must set the supplied score
    /// path to one as its final measured command. The runner initializes that score
    /// to zero and observes it later from a separate console root. Zero is a valid
    /// configured limit because vanilla accepts zero-valued command-limit gamerules.
    #[must_use]
    pub const fn new(
        kind: ExactLimitKind,
        configured_limit: u32,
        completion: ObservationPath,
        root_completes: bool,
    ) -> Self {
        Self {
            kind,
            configured_limit,
            completion,
            root_completes,
        }
    }

    /// Returns the hard limit selected by this probe.
    #[must_use]
    pub const fn kind(&self) -> ExactLimitKind {
        self.kind
    }

    /// Returns the exact gamerule value used for the measured root.
    #[must_use]
    pub const fn configured_limit(&self) -> u32 {
        self.configured_limit
    }

    /// Returns the score path written last by the measured root.
    #[must_use]
    pub const fn completion(&self) -> &ObservationPath {
        &self.completion
    }

    /// Returns whether the final measured completion write must execute.
    #[must_use]
    pub const fn root_completes(&self) -> bool {
        self.root_completes
    }
}

/// Scenario protocol; variants prevent semantic wrappers from entering limit probes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScenarioMode {
    /// State semantics under generous hard limits.
    Semantic {
        /// Optional independent success/result/continuation contract.
        outcome: Option<CommandOutcomeExpectation>,
    },
    /// Bare invocation followed by observation from a new console root.
    ExactLimit(ExactLimitContract),
    /// Environment-specific measurement protocol, never an ordinary assertion.
    Measurement {
        /// Registered measurement protocol identity.
        protocol: TestName,
    },
}

impl Default for ScenarioMode {
    fn default() -> Self {
        Self::Semantic { outcome: None }
    }
}

/// World resource explicitly owned and cleaned by one scenario.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OwnedResource {
    /// Scoreboard objective.
    Objective(TestName),
    /// Command storage.
    Storage(ResourceLocation),
    /// Entity ownership tag.
    EntityTag(TestName),
    /// Exact test-owned block.
    Block {
        /// Dimension.
        dimension: ResourceLocation,
        /// Position.
        position: BlockPosition,
    },
    /// Chunk held loaded for controlled entities/blocks.
    ForcedChunk {
        /// Dimension.
        dimension: ResourceLocation,
        /// Chunk x coordinate.
        x: i32,
        /// Chunk z coordinate.
        z: i32,
    },
    /// Test-owned bossbar.
    Bossbar(ResourceLocation),
    /// Gamerule whose previous value must be restored by cleanup.
    Gamerule(Box<str>),
}

/// Mutable declaration assembled before validation.
#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioSpec {
    id: ScenarioId,
    generation: GenerationNonce,
    entry: ScenarioEntry,
    policies: Vec<CompilerPolicy>,
    mode: ScenarioMode,
    invocation: InvocationFrame,
    owned: Vec<OwnedResource>,
    cleanup: Vec<OwnedResource>,
    initial: ObservationSet,
    expected: ObservationSet,
}

impl ScenarioSpec {
    /// Creates a semantic scenario using all four policy products.
    #[must_use]
    pub fn new(id: ScenarioId, generation: GenerationNonce, entry: ScenarioEntry) -> Self {
        Self {
            id,
            generation,
            entry,
            policies: CompilerPolicy::ALL.to_vec(),
            mode: ScenarioMode::default(),
            invocation: InvocationFrame::default(),
            owned: vec![],
            cleanup: vec![],
            initial: ObservationSet::empty(),
            expected: ObservationSet::empty(),
        }
    }

    /// Replaces the policy matrix.
    #[must_use]
    pub fn with_policies(mut self, policies: impl Into<Vec<CompilerPolicy>>) -> Self {
        self.policies = policies.into();
        self
    }

    /// Replaces the execution protocol.
    #[must_use]
    pub fn with_mode(mut self, mode: ScenarioMode) -> Self {
        self.mode = mode;
        self
    }

    /// Replaces the explicit invocation frame.
    #[must_use]
    pub fn with_invocation(mut self, invocation: InvocationFrame) -> Self {
        self.invocation = invocation;
        self
    }

    /// Declares resources owned by setup.
    #[must_use]
    pub fn with_owned_resources(mut self, owned: impl Into<Vec<OwnedResource>>) -> Self {
        self.owned = owned.into();
        self
    }

    /// Declares resources cleanup will remove or restore.
    #[must_use]
    pub fn with_cleanup(mut self, cleanup: impl Into<Vec<OwnedResource>>) -> Self {
        self.cleanup = cleanup.into();
        self
    }

    /// Replaces the initial controlled-state projection.
    #[must_use]
    pub fn with_initial(mut self, initial: ObservationSet) -> Self {
        self.initial = initial;
        self
    }

    /// Replaces the expected final projection.
    #[must_use]
    pub fn with_expected(mut self, expected: ObservationSet) -> Self {
        self.expected = expected;
        self
    }

    /// Validates and freezes the scenario contract before server startup.
    ///
    /// # Errors
    ///
    /// Returns all locally detectable ownership, policy, and path errors.
    pub fn build(self) -> Result<MinecraftScenario, ScenarioValidationError> {
        MinecraftScenario::validate(self)
    }
}

/// Fully validated scenario safe to hand to a server runner.
#[derive(Clone, Debug, PartialEq)]
pub struct MinecraftScenario {
    spec: ScenarioSpec,
    result_root: ResourceLocation,
}

impl MinecraftScenario {
    fn validate(spec: ScenarioSpec) -> Result<Self, ScenarioValidationError> {
        validate_policies(&spec.policies)?;
        let owned = collect_unique(&spec.owned, true)?;
        let cleanup = collect_unique(&spec.cleanup, false)?;
        for resource in &cleanup {
            if !owned.contains(resource) {
                return Err(ScenarioValidationError::UndeclaredCleanup(resource.clone()));
            }
        }
        for resource in &owned {
            validate_owned_resource(resource)?;
        }
        validate_projection_ownership(&spec.initial, &owned)?;
        validate_projection_ownership(&spec.expected, &owned)?;
        if let ScenarioMode::ExactLimit(contract) = &spec.mode {
            if !matches!(contract.completion(), ObservationPath::Score { .. }) {
                return Err(ScenarioValidationError::ExactLimitCompletionNotScore(
                    contract.completion().clone(),
                ));
            }
            validate_projection_ownership(
                &ObservationSet::try_new([(
                    contract.completion().clone(),
                    ObservationValue::Score(Some(i32::from(contract.root_completes()))),
                )])?,
                &owned,
            )?;
            if spec.initial.get(contract.completion()).is_some()
                || spec.expected.get(contract.completion()).is_some()
            {
                return Err(ScenarioValidationError::ExactLimitCompletionProjected(
                    contract.completion().clone(),
                ));
            }
        }
        let result_root = ResourceLocation::new(
            "mdl_test",
            format!("result/{}/{:016x}", spec.id.as_str(), spec.generation.get()),
        )?;
        Ok(Self { spec, result_root })
    }

    /// Returns the stable scenario identity.
    #[must_use]
    pub const fn id(&self) -> &ScenarioId {
        &self.spec.id
    }

    /// Returns the generation nonce.
    #[must_use]
    pub const fn generation(&self) -> GenerationNonce {
        self.spec.generation
    }

    /// Returns policies in deterministic execution order.
    #[must_use]
    pub fn policies(&self) -> &[CompilerPolicy] {
        &self.spec.policies
    }

    /// Returns the logical entry operation.
    #[must_use]
    pub const fn entry(&self) -> &ScenarioEntry {
        &self.spec.entry
    }

    /// Returns the selected instrumentation protocol.
    #[must_use]
    pub const fn mode(&self) -> &ScenarioMode {
        &self.spec.mode
    }

    /// Returns the explicit incoming execution frame.
    #[must_use]
    pub const fn invocation(&self) -> &InvocationFrame {
        &self.spec.invocation
    }

    /// Returns setup-owned world resources.
    #[must_use]
    pub fn owned_resources(&self) -> &[OwnedResource] {
        &self.spec.owned
    }

    /// Returns resources removed or restored after execution.
    #[must_use]
    pub fn cleanup_resources(&self) -> &[OwnedResource] {
        &self.spec.cleanup
    }

    /// Returns the initial controlled-state projection.
    #[must_use]
    pub const fn initial(&self) -> &ObservationSet {
        &self.spec.initial
    }

    /// Returns the expected final projection.
    #[must_use]
    pub const fn expected(&self) -> &ObservationSet {
        &self.spec.expected
    }

    /// Derives a stable generation-specific result storage root.
    #[must_use]
    pub const fn result_root(&self) -> &ResourceLocation {
        &self.result_root
    }

    /// Renders a concise deterministic contract for failure reports.
    #[must_use]
    pub fn render(&self) -> String {
        let policies = self
            .spec
            .policies
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        let observations = self
            .spec
            .expected
            .iter()
            .map(|(path, value)| format!("  {path} = {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "scenario {} generation={}\nmode={:?}\npolicies=[{}]\nentry={:?}\nexpected:\n{}",
            self.spec.id,
            self.spec.generation.get(),
            self.spec.mode,
            policies,
            self.spec.entry,
            observations
        )
    }
}

/// Invalid typed scenario detected without Java or filesystem mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScenarioValidationError {
    /// Invalid safe name.
    Name(NameError),
    /// Invalid normalized observation set.
    Observation(ObservationSetError),
    /// At least one compilation policy is required.
    NoPolicies,
    /// A policy product was scheduled twice.
    DuplicatePolicy(CompilerPolicy),
    /// Setup declared one owned resource twice.
    DuplicateOwnedResource(OwnedResource),
    /// Cleanup declared one resource twice.
    DuplicateCleanup(OwnedResource),
    /// Cleanup attempted to mutate a resource not declared by setup.
    UndeclaredCleanup(OwnedResource),
    /// A projected path does not belong to a declared test resource.
    ObservationNotOwned(ObservationPath),
    /// Entity comparison mode disagrees with its normalized collection.
    EntityComparisonMismatch(ObservationPath),
    /// Selector text is not a safe single command argument.
    UnsafeSelector(String),
    /// Gamerule text is not a safe command word.
    UnsafeGamerule(String),
    /// A coordinate was NaN or infinite.
    NonFiniteCoordinate,
    /// Local position coordinates cannot be mixed with world/relative axes.
    MixedLocalCoordinates,
    /// Rotation supports absolute and relative coordinates, not local axes.
    LocalRotation,
    /// Exact-limit completion must use a score path so the bare runner can reset it.
    ExactLimitCompletionNotScore(ObservationPath),
    /// The exact-limit contract, not the ordinary state projection, owns completion.
    ExactLimitCompletionProjected(ObservationPath),
}

impl fmt::Display for ScenarioValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(error) => write!(formatter, "{error}"),
            Self::Observation(error) => write!(formatter, "{error}"),
            Self::NoPolicies => formatter.write_str("scenario has no compiler policies"),
            Self::DuplicatePolicy(policy) => write!(formatter, "duplicate policy {policy}"),
            Self::DuplicateOwnedResource(resource) => {
                write!(formatter, "duplicate setup ownership {resource:?}")
            }
            Self::DuplicateCleanup(resource) => {
                write!(formatter, "duplicate cleanup declaration {resource:?}")
            }
            Self::UndeclaredCleanup(resource) => {
                write!(formatter, "cleanup does not own {resource:?}")
            }
            Self::ObservationNotOwned(path) => {
                write!(formatter, "observation path is not test-owned: {path}")
            }
            Self::EntityComparisonMismatch(path) => {
                write!(formatter, "entity comparison mode disagrees at {path}")
            }
            Self::UnsafeSelector(selector) => write!(formatter, "unsafe selector {selector:?}"),
            Self::UnsafeGamerule(gamerule) => write!(formatter, "unsafe gamerule {gamerule:?}"),
            Self::NonFiniteCoordinate => formatter.write_str("coordinate must be finite"),
            Self::MixedLocalCoordinates => {
                formatter.write_str("local coordinates must be used on all three position axes")
            }
            Self::LocalRotation => formatter.write_str("rotation cannot use local coordinates"),
            Self::ExactLimitCompletionNotScore(path) => {
                write!(
                    formatter,
                    "exact-limit completion is not a score path: {path}"
                )
            }
            Self::ExactLimitCompletionProjected(path) => write!(
                formatter,
                "exact-limit completion must not be duplicated in state projections: {path}"
            ),
        }
    }
}

impl Error for ScenarioValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Name(error) => Some(error),
            Self::Observation(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NameError> for ScenarioValidationError {
    fn from(error: NameError) -> Self {
        Self::Name(error)
    }
}

impl From<ObservationSetError> for ScenarioValidationError {
    fn from(error: ObservationSetError) -> Self {
        Self::Observation(error)
    }
}

fn validate_coordinates(coordinates: [Coordinate; 3]) -> Result<(), ScenarioValidationError> {
    if coordinates
        .iter()
        .any(|coordinate| !coordinate.value().is_finite())
    {
        return Err(ScenarioValidationError::NonFiniteCoordinate);
    }
    let local_count = coordinates
        .iter()
        .filter(|coordinate| coordinate.is_local())
        .count();
    if local_count != 0 && local_count != coordinates.len() {
        return Err(ScenarioValidationError::MixedLocalCoordinates);
    }
    Ok(())
}

fn validate_policies(policies: &[CompilerPolicy]) -> Result<(), ScenarioValidationError> {
    if policies.is_empty() {
        return Err(ScenarioValidationError::NoPolicies);
    }
    let mut seen = BTreeSet::new();
    for policy in policies {
        if !seen.insert(*policy) {
            return Err(ScenarioValidationError::DuplicatePolicy(*policy));
        }
    }
    Ok(())
}

fn collect_unique(
    resources: &[OwnedResource],
    setup: bool,
) -> Result<BTreeSet<OwnedResource>, ScenarioValidationError> {
    let mut seen = BTreeSet::new();
    for resource in resources {
        if !seen.insert(resource.clone()) {
            return Err(if setup {
                ScenarioValidationError::DuplicateOwnedResource(resource.clone())
            } else {
                ScenarioValidationError::DuplicateCleanup(resource.clone())
            });
        }
    }
    Ok(seen)
}

fn validate_owned_resource(resource: &OwnedResource) -> Result<(), ScenarioValidationError> {
    if let OwnedResource::Gamerule(gamerule) = resource
        && (gamerule.is_empty()
            || !gamerule
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
    {
        return Err(ScenarioValidationError::UnsafeGamerule(
            gamerule.to_string(),
        ));
    }
    Ok(())
}

fn validate_projection_ownership(
    observations: &ObservationSet,
    owned: &BTreeSet<OwnedResource>,
) -> Result<(), ScenarioValidationError> {
    for (path, value) in observations.iter() {
        let required = match path {
            ObservationPath::Score { objective, .. } => {
                Some(OwnedResource::Objective(objective.clone()))
            }
            ObservationPath::Storage { storage, .. } => {
                Some(OwnedResource::Storage(storage.clone()))
            }
            ObservationPath::Block {
                dimension,
                position,
            } => Some(OwnedResource::Block {
                dimension: dimension.clone(),
                position: *position,
            }),
            ObservationPath::Entities { tag, comparison } => {
                validate_entity_comparison(path, *comparison, value)?;
                Some(OwnedResource::EntityTag(tag.clone()))
            }
            ObservationPath::LogEffects { .. } => None,
        };
        if required.is_some_and(|required| !owned.contains(&required)) {
            return Err(ScenarioValidationError::ObservationNotOwned(path.clone()));
        }
    }
    Ok(())
}

fn validate_entity_comparison(
    path: &ObservationPath,
    comparison: EntityComparison,
    value: &ObservationValue,
) -> Result<(), ScenarioValidationError> {
    let matches = matches!(
        (comparison, value),
        (
            EntityComparison::Set,
            ObservationValue::Entities(EntityCollection::Set(_))
        ) | (
            EntityComparison::Multiset,
            ObservationValue::Entities(EntityCollection::Multiset(_))
        )
    );
    if matches {
        Ok(())
    } else {
        Err(ScenarioValidationError::EntityComparisonMismatch(
            path.clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompilerPolicy, Coordinate, EntitySelector, ExactLimitContract, ExactLimitKind,
        InvocationFrame, MinecraftScenario, OwnedResource, Position, ScenarioEntry, ScenarioMode,
        ScenarioSpec,
    };
    use crate::scenario::{
        GenerationNonce, ObservationPath, ObservationSet, ObservationValue, ResourceLocation,
        ScenarioId, TestName,
    };

    fn base_spec() -> ScenarioSpec {
        ScenarioSpec::new(
            ScenarioId::new("scalar/identity").unwrap(),
            GenerationNonce::new(7).unwrap(),
            ScenarioEntry::Export(TestName::new("identity").unwrap()),
        )
    }

    #[test]
    fn default_scenario_has_four_policies_and_stable_result_root() {
        let scenario = base_spec().build().unwrap();
        assert_eq!(scenario.policies(), CompilerPolicy::ALL);
        assert_eq!(
            scenario.result_root().to_string(),
            "mdl_test:result/scalar/identity/0000000000000007"
        );
        assert_eq!(scenario.render(), scenario.render());
    }

    #[test]
    fn setup_ownership_is_checked_before_server_startup() {
        let objective = TestName::new("mdl.case").unwrap();
        let path = ObservationPath::Score {
            holder: "#result".into(),
            objective: objective.clone(),
        };
        let expected =
            ObservationSet::try_new([(path, ObservationValue::Score(Some(42)))]).unwrap();
        let error = base_spec().with_expected(expected).build().unwrap_err();
        assert!(error.to_string().contains("not test-owned"));

        let resource = OwnedResource::Objective(objective);
        let valid = base_spec()
            .with_owned_resources(vec![resource.clone()])
            .with_cleanup(vec![resource])
            .build();
        assert!(valid.is_ok());
    }

    #[test]
    fn duplicate_and_undeclared_cleanup_are_rejected() {
        let storage = OwnedResource::Storage(ResourceLocation::parse("mdl_test:state").unwrap());
        assert!(
            base_spec()
                .with_owned_resources(vec![storage.clone(), storage.clone()])
                .build()
                .unwrap_err()
                .to_string()
                .contains("duplicate setup")
        );
        assert!(
            base_spec()
                .with_cleanup(vec![storage])
                .build()
                .unwrap_err()
                .to_string()
                .contains("does not own")
        );
    }

    #[test]
    fn exact_limit_mode_cannot_carry_semantic_outcome_instrumentation() {
        let objective = TestName::new("mdl.limit").unwrap();
        let completion = ObservationPath::Score {
            holder: "#completed".into(),
            objective: objective.clone(),
        };
        let scenario = base_spec()
            .with_mode(ScenarioMode::ExactLimit(ExactLimitContract::new(
                ExactLimitKind::Sequence,
                0,
                completion.clone(),
                false,
            )))
            .with_owned_resources(vec![OwnedResource::Objective(objective.clone())])
            .build()
            .unwrap();
        let rendered = scenario.render();
        assert!(rendered.contains("ExactLimit"));
        assert!(!rendered.contains("outcome: Some"));

        let duplicated =
            ObservationSet::try_new([(completion.clone(), ObservationValue::Score(Some(0)))])
                .unwrap();
        let error = base_spec()
            .with_mode(ScenarioMode::ExactLimit(ExactLimitContract::new(
                ExactLimitKind::Sequence,
                0,
                completion,
                false,
            )))
            .with_owned_resources(vec![OwnedResource::Objective(objective)])
            .with_expected(duplicated)
            .build()
            .unwrap_err();
        assert!(error.to_string().contains("must not be duplicated"));
    }

    #[test]
    fn positions_and_selectors_reject_ambiguous_or_injectable_frames() {
        assert!(
            Position::new(
                Coordinate::Local(0.0),
                Coordinate::Relative(0.0),
                Coordinate::Local(0.0),
            )
            .is_err()
        );
        assert!(
            Position::new(
                Coordinate::Absolute(f64::NAN),
                Coordinate::Absolute(0.0),
                Coordinate::Absolute(0.0),
            )
            .is_err()
        );
        assert!(EntitySelector::new("@e\nsay injected").is_err());

        let selector = EntitySelector::new("@e[tag=mdl_case,limit=1]").unwrap();
        let frame = InvocationFrame::server()
            .with_executor(super::Executor::Entities(selector))
            .with_position(
                Position::new(
                    Coordinate::Relative(10.0),
                    Coordinate::Relative(0.0),
                    Coordinate::Relative(0.0),
                )
                .unwrap(),
            );
        assert!(base_spec().with_invocation(frame).build().is_ok());
    }

    #[test]
    fn invalid_gamerule_is_rejected() {
        let error = base_spec()
            .with_owned_resources(vec![OwnedResource::Gamerule("bad rule".into())])
            .build()
            .unwrap_err();
        assert!(error.to_string().contains("unsafe gamerule"));
    }

    #[test]
    fn validated_scenario_type_remains_opaque() {
        fn accepts_validated(_: &MinecraftScenario) {}
        let scenario = base_spec().build().unwrap();
        accepts_validated(&scenario);
    }
}
