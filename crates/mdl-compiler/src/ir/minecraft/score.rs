use std::error::Error;
use std::fmt;
use std::str::FromStr;

use super::{AtMostOneSelector, Selector};

/// The score-name domain in which validation failed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScoreNameKind {
    /// A scoreboard objective name.
    Objective,
    /// The payload of a compiler-owned fake score holder.
    FakeHolder,
}

impl fmt::Display for ScoreNameKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Objective => "objective name",
            Self::FakeHolder => "fake score holder",
        })
    }
}

/// Why a score name was rejected.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScoreNameErrorReason {
    /// The name or fake-holder payload was empty.
    Empty,
    /// A fake holder did not start with `#`.
    MissingFakeHolderPrefix,
    /// A character is outside the compiler's conservative score-word alphabet.
    InvalidCharacter {
        /// Byte offset within the objective or fake-holder payload.
        byte_index: usize,
        /// The rejected Unicode scalar value.
        character: char,
    },
}

/// A failed objective or fake-holder validation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScoreNameError {
    kind: ScoreNameKind,
    reason: ScoreNameErrorReason,
}

impl ScoreNameError {
    const fn new(kind: ScoreNameKind, reason: ScoreNameErrorReason) -> Self {
        Self { kind, reason }
    }

    /// Returns the score-name domain that failed.
    #[must_use]
    pub const fn kind(self) -> ScoreNameKind {
        self.kind
    }

    /// Returns the precise rejection reason.
    #[must_use]
    pub const fn reason(self) -> ScoreNameErrorReason {
        self.reason
    }
}

impl fmt::Display for ScoreNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid {}: ", self.kind)?;
        match self.reason {
            ScoreNameErrorReason::Empty => formatter.write_str("value cannot be empty"),
            ScoreNameErrorReason::MissingFakeHolderPrefix => {
                formatter.write_str("value must start with `#`")
            }
            ScoreNameErrorReason::InvalidCharacter {
                byte_index,
                character,
            } => write!(
                formatter,
                "character {character:?} at byte {byte_index} is not accepted"
            ),
        }
    }
}

impl Error for ScoreNameError {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ScoreWord(Box<str>);

impl ScoreWord {
    fn new(value: &str, kind: ScoreNameKind) -> Result<Self, ScoreNameError> {
        validate_score_word(value, kind)?;
        Ok(Self(value.into()))
    }

    fn from_string(value: String, kind: ScoreNameKind) -> Result<Self, ScoreNameError> {
        validate_score_word(&value, kind)?;
        Ok(Self(value.into_boxed_str()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated scoreboard objective name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObjectiveName(ScoreWord);

impl ObjectiveName {
    /// Validates and owns an objective name.
    ///
    /// # Errors
    ///
    /// Rejects empty values and characters outside ASCII letters, digits, `_`,
    /// `-`, `.`, and `+`.
    pub fn new(value: &str) -> Result<Self, ScoreNameError> {
        ScoreWord::new(value, ScoreNameKind::Objective).map(Self)
    }

    /// Returns the validated objective text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for ObjectiveName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ObjectiveName {
    type Err = ScoreNameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for ObjectiveName {
    type Error = ScoreNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        ScoreWord::from_string(value, ScoreNameKind::Objective).map(Self)
    }
}

/// A validated compiler-owned fake score holder.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FakeScoreHolder(ScoreWord);

impl FakeScoreHolder {
    /// Validates and owns a fake holder including its required leading `#`.
    ///
    /// # Errors
    ///
    /// Rejects a missing prefix, an empty payload, or a payload character outside
    /// the conservative score-word alphabet.
    pub fn new(value: &str) -> Result<Self, ScoreNameError> {
        let payload = value.strip_prefix('#').ok_or_else(|| {
            ScoreNameError::new(
                ScoreNameKind::FakeHolder,
                ScoreNameErrorReason::MissingFakeHolderPrefix,
            )
        })?;
        ScoreWord::new(payload, ScoreNameKind::FakeHolder).map(Self)
    }

    /// Returns the validated payload without its rendered leading `#`.
    #[must_use]
    pub fn payload(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for FakeScoreHolder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "#{}", self.payload())
    }
}

impl FromStr for FakeScoreHolder {
    type Err = ScoreNameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for FakeScoreHolder {
    type Error = ScoreNameError;

    fn try_from(mut value: String) -> Result<Self, Self::Error> {
        if !value.starts_with('#') {
            return Err(ScoreNameError::new(
                ScoreNameKind::FakeHolder,
                ScoreNameErrorReason::MissingFakeHolderPrefix,
            ));
        }
        value.remove(0);
        ScoreWord::from_string(value, ScoreNameKind::FakeHolder).map(Self)
    }
}

/// A nonnegative `i32` accepted by scoreboard add/remove.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NonNegativeI32(i32);

impl NonNegativeI32 {
    /// Validates a scoreboard add/remove amount.
    ///
    /// # Errors
    ///
    /// Returns [`NegativeScoreAmount`] when `value` is negative.
    pub const fn new(value: i32) -> Result<Self, NegativeScoreAmount> {
        if value < 0 {
            Err(NegativeScoreAmount(value))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated amount.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

impl TryFrom<i32> for NonNegativeI32 {
    type Error = NegativeScoreAmount;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<NonNegativeI32> for i32 {
    fn from(value: NonNegativeI32) -> Self {
        value.get()
    }
}

impl fmt::Display for NonNegativeI32 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A negative value supplied where scoreboard syntax requires a nonnegative `i32`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NegativeScoreAmount(i32);

impl NegativeScoreAmount {
    /// Returns the rejected value.
    #[must_use]
    pub const fn value(self) -> i32 {
        self.0
    }
}

impl fmt::Display for NegativeScoreAmount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "score amount {} cannot be negative", self.0)
    }
}

impl Error for NegativeScoreAmount {}

/// The canonical shape of an inclusive integer score range.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScoreRangeKind {
    /// One exact score.
    Exact(i32),
    /// Scores greater than or equal to the bound.
    AtLeast(i32),
    /// Scores less than or equal to the bound.
    AtMost(i32),
    /// Scores inclusively between two distinct ordered bounds.
    Between {
        /// Inclusive lower bound.
        min: i32,
        /// Inclusive upper bound.
        max: i32,
    },
}

/// A nonempty, ordered inclusive integer range for `execute ... matches`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScoreRange(ScoreRangeKind);

impl ScoreRange {
    /// Constructs one exact score.
    #[must_use]
    pub const fn exact(value: i32) -> Self {
        Self(ScoreRangeKind::Exact(value))
    }

    /// Constructs a lower-bounded score range.
    #[must_use]
    pub const fn at_least(min: i32) -> Self {
        Self(ScoreRangeKind::AtLeast(min))
    }

    /// Constructs an upper-bounded score range.
    #[must_use]
    pub const fn at_most(max: i32) -> Self {
        Self(ScoreRangeKind::AtMost(max))
    }

    /// Constructs an inclusive closed score range.
    ///
    /// Equal bounds canonicalize to [`Self::exact`].
    ///
    /// # Errors
    ///
    /// Rejects a lower bound greater than the upper bound.
    pub const fn between(min: i32, max: i32) -> Result<Self, BackwardsScoreRange> {
        if min > max {
            Err(BackwardsScoreRange { min, max })
        } else if min == max {
            Ok(Self::exact(min))
        } else {
            Ok(Self(ScoreRangeKind::Between { min, max }))
        }
    }

    /// Returns the canonical range shape.
    #[must_use]
    pub const fn kind(self) -> ScoreRangeKind {
        self.0
    }
}

impl fmt::Display for ScoreRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ScoreRangeKind::Exact(value) => value.fmt(formatter),
            ScoreRangeKind::AtLeast(min) => write!(formatter, "{min}.."),
            ScoreRangeKind::AtMost(max) => write!(formatter, "..{max}"),
            ScoreRangeKind::Between { min, max } => write!(formatter, "{min}..{max}"),
        }
    }
}

/// Reversed bounds supplied to [`ScoreRange::between`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackwardsScoreRange {
    min: i32,
    max: i32,
}

impl BackwardsScoreRange {
    /// Returns the rejected lower bound.
    #[must_use]
    pub const fn min(self) -> i32 {
        self.min
    }

    /// Returns the rejected upper bound.
    #[must_use]
    pub const fn max(self) -> i32 {
        self.max
    }
}

impl fmt::Display for BackwardsScoreRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "score range lower bound {} exceeds upper bound {}",
            self.min, self.max
        )
    }
}

impl Error for BackwardsScoreRange {}

fn validate_score_word(value: &str, kind: ScoreNameKind) -> Result<(), ScoreNameError> {
    if value.is_empty() {
        return Err(ScoreNameError::new(kind, ScoreNameErrorReason::Empty));
    }
    if let Some((byte_index, character)) = value
        .char_indices()
        .find(|(_, character)| !is_score_word_character(*character))
    {
        return Err(ScoreNameError::new(
            kind,
            ScoreNameErrorReason::InvalidCharacter {
                byte_index,
                character,
            },
        ));
    }
    Ok(())
}

const fn is_score_word_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '+')
}

/// A score-holder argument known to resolve to at most one score.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum SingleScoreHolder {
    /// One stable compiler-owned fake holder.
    Fake(FakeScoreHolder),
    /// One selector with statically bounded cardinality.
    Selector(AtMostOneSelector),
}

impl From<FakeScoreHolder> for SingleScoreHolder {
    fn from(holder: FakeScoreHolder) -> Self {
        Self::Fake(holder)
    }
}

impl From<AtMostOneSelector> for SingleScoreHolder {
    fn from(selector: AtMostOneSelector) -> Self {
        Self::Selector(selector)
    }
}

/// Any score-holder argument accepted by native bulk player-score commands.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ScoreHolders {
    /// One stable compiler-owned fake holder.
    Fake(FakeScoreHolder),
    /// A selector with its native target cardinality.
    Selector(Selector),
    /// Every holder tracked anywhere by the scoreboard service (`*`).
    AllTracked,
}

impl From<FakeScoreHolder> for ScoreHolders {
    fn from(holder: FakeScoreHolder) -> Self {
        Self::Fake(holder)
    }
}

impl From<AtMostOneSelector> for ScoreHolders {
    fn from(selector: AtMostOneSelector) -> Self {
        Self::Selector(selector.into())
    }
}

impl From<Selector> for ScoreHolders {
    fn from(selector: Selector) -> Self {
        Self::Selector(selector)
    }
}

/// One native score-holder selection paired with an objective.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ScoreSelection {
    holders: ScoreHolders,
    objective: ObjectiveName,
}

impl ScoreSelection {
    /// Constructs a native scalar or bulk score selection.
    #[must_use]
    pub const fn new(holders: ScoreHolders, objective: ObjectiveName) -> Self {
        Self { holders, objective }
    }

    /// Returns the selected holders.
    #[must_use]
    pub const fn holders(&self) -> &ScoreHolders {
        &self.holders
    }

    /// Returns the objective.
    #[must_use]
    pub const fn objective(&self) -> &ObjectiveName {
        &self.objective
    }
}

/// Exactly one syntactically addressable scoreboard value.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ScoreRef {
    holder: SingleScoreHolder,
    objective: ObjectiveName,
}

impl ScoreRef {
    /// Constructs a scalar score reference.
    #[must_use]
    pub const fn new(holder: SingleScoreHolder, objective: ObjectiveName) -> Self {
        Self { holder, objective }
    }

    /// Returns the scalar holder.
    #[must_use]
    pub const fn holder(&self) -> &SingleScoreHolder {
        &self.holder
    }

    /// Returns the objective.
    #[must_use]
    pub const fn objective(&self) -> &ObjectiveName {
        &self.objective
    }
}

/// A native scoreboard player operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScoreOperation {
    /// `=`
    Assign,
    /// `+=`
    Add,
    /// `-=`
    Subtract,
    /// `*=`
    Multiply,
    /// `/=`
    Divide,
    /// `%=`
    Modulo,
    /// `<`
    Min,
    /// `>`
    Max,
    /// `><`
    Swap,
}

/// The closed initial scoreboard command vocabulary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ScoreCommand {
    /// Create one dummy objective.
    ObjectiveAddDummy { objective: ObjectiveName },
    /// Set every selected score to a literal value.
    PlayersSet { target: ScoreSelection, value: i32 },
    /// Add a nonnegative literal to every selected score.
    PlayersAdd {
        target: ScoreSelection,
        amount: NonNegativeI32,
    },
    /// Remove a nonnegative literal from every selected score.
    PlayersRemove {
        target: ScoreSelection,
        amount: NonNegativeI32,
    },
    /// Read exactly one score.
    PlayersGet { score: ScoreRef },
    /// Reset every selected score.
    PlayersReset { target: ScoreSelection },
    /// Apply one source selection sequentially to every target selection.
    PlayersOperation {
        target: ScoreSelection,
        op: ScoreOperation,
        source: ScoreSelection,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        FakeScoreHolder, NonNegativeI32, ObjectiveName, ScoreCommand, ScoreHolders,
        ScoreNameErrorReason, ScoreNameKind, ScoreOperation, ScoreRange, ScoreRangeKind, ScoreRef,
        ScoreSelection, SingleScoreHolder,
    };
    use crate::ir::minecraft::{AtMostOneSelector, Selector, UnboundedSelector};

    #[test]
    fn objective_and_fake_holder_share_only_the_private_word_grammar() {
        for accepted in ["A", "mdl.reg-0", "+temporary+"] {
            assert_eq!(ObjectiveName::new(accepted).unwrap().as_str(), accepted);
        }
        assert_eq!(
            ObjectiveName::new("").unwrap_err().reason(),
            ScoreNameErrorReason::Empty
        );
        let invalid = ObjectiveName::new("bad/name").unwrap_err();
        assert_eq!(invalid.kind(), ScoreNameKind::Objective);
        assert_eq!(
            invalid.reason(),
            ScoreNameErrorReason::InvalidCharacter {
                byte_index: 3,
                character: '/'
            }
        );

        let holder = FakeScoreHolder::new("#mdl.tmp+0").unwrap();
        assert_eq!(holder.payload(), "mdl.tmp+0");
        assert_eq!(holder.to_string(), "#mdl.tmp+0");
        assert_eq!(
            FakeScoreHolder::new("mdl.tmp").unwrap_err().reason(),
            ScoreNameErrorReason::MissingFakeHolderPrefix
        );
        assert_eq!(
            FakeScoreHolder::new("#").unwrap_err().reason(),
            ScoreNameErrorReason::Empty
        );
        assert!(ObjectiveName::new("#mdl.tmp").is_err());
    }

    #[test]
    fn owned_score_names_do_not_require_a_second_allocation() {
        let objective = ObjectiveName::try_from(String::from("mdl.reg")).unwrap();
        let holder = FakeScoreHolder::try_from(String::from("#mdl.tmp")).unwrap();

        assert_eq!(objective.as_str(), "mdl.reg");
        assert_eq!(holder.payload(), "mdl.tmp");
    }

    #[test]
    fn nonnegative_i32_matches_the_brigadier_amount_domain() {
        assert_eq!(NonNegativeI32::new(0).unwrap().get(), 0);
        assert_eq!(NonNegativeI32::new(i32::MAX).unwrap().get(), i32::MAX);
        let error = NonNegativeI32::new(-1).unwrap_err();
        assert_eq!(error.value(), -1);
    }

    #[test]
    fn score_ranges_have_only_canonical_valid_shapes() {
        let cases = [
            (ScoreRange::exact(-3), "-3"),
            (ScoreRange::at_least(i32::MIN), "-2147483648.."),
            (ScoreRange::at_most(i32::MAX), "..2147483647"),
            (ScoreRange::between(-4, 9).unwrap(), "-4..9"),
        ];
        for (range, expected) in cases {
            assert_eq!(range.to_string(), expected);
        }

        assert_eq!(
            ScoreRange::between(7, 7).unwrap().kind(),
            ScoreRangeKind::Exact(7)
        );
        let error = ScoreRange::between(8, 7).unwrap_err();
        assert_eq!((error.min(), error.max()), (8, 7));
    }

    #[test]
    fn scalar_and_bulk_score_domains_are_structurally_distinct() {
        let objective = ObjectiveName::new("mdl.reg").unwrap();
        let scalar = ScoreRef::new(
            SingleScoreHolder::from(AtMostOneSelector::SelfExecutor),
            objective.clone(),
        );
        assert!(matches!(
            scalar.holder(),
            SingleScoreHolder::Selector(AtMostOneSelector::SelfExecutor)
        ));

        let bulk = ScoreSelection::new(
            ScoreHolders::from(Selector::from(UnboundedSelector::AllEntities)),
            objective,
        );
        assert!(matches!(
            bulk.holders(),
            ScoreHolders::Selector(Selector::Unbounded(UnboundedSelector::AllEntities))
        ));
    }

    #[test]
    fn native_multi_holder_operation_remains_one_ordered_command() {
        let objective = ObjectiveName::new("mdl.reg").unwrap();
        let target = ScoreSelection::new(ScoreHolders::AllTracked, objective.clone());
        let source = ScoreSelection::new(
            ScoreHolders::from(Selector::from(UnboundedSelector::AllPlayers)),
            objective,
        );
        let command = ScoreCommand::PlayersOperation {
            target,
            op: ScoreOperation::Add,
            source,
        };

        assert!(matches!(
            command,
            ScoreCommand::PlayersOperation {
                op: ScoreOperation::Add,
                ..
            }
        ));
    }
}
