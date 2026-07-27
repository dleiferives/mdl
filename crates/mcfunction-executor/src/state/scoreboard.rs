use std::collections::HashMap;

/// Scoreboard engine: maps (holder, objective) to i32 values.
///
/// Matches wrapping-signed-32 semantics for all arithmetic.
#[derive(Debug, Clone, Default)]
pub struct ScoreboardEngine {
    scores: HashMap<(ScoreHolderKey, String), i32>,
}

impl ScoreboardEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, holder: &str, objective: &str, value: i32) {
        self.scores.insert(
            (ScoreHolderKey::from_str(holder), objective.to_owned()),
            value,
        );
    }

    pub fn get(&self, holder: &str, objective: &str) -> Option<i32> {
        self.scores
            .get(&(ScoreHolderKey::from_str(holder), objective.to_owned()))
            .copied()
    }

    /// Returns all (holder, objective, score) entries for holders matching the
    /// given key prefix (e.g. `#mdl` or `@e`).
    pub fn matching_holders(&self, holder_pattern: &str) -> Vec<(String, i32)> {
        let mut results = Vec::new();
        for ((holder_key, obj), score) in &self.scores {
            if holder_key.matches(holder_pattern) {
                results.push((obj.clone(), *score));
            }
        }
        results
    }

    pub fn add(&mut self, holder: &str, objective: &str, amount: i32) {
        let key = (ScoreHolderKey::from_str(holder), objective.to_owned());
        let current = self.scores.get(&key).copied().unwrap_or(0);
        self.scores.insert(key, current.wrapping_add(amount));
    }

    pub fn remove_amount(&mut self, holder: &str, objective: &str, amount: i32) {
        let key = (ScoreHolderKey::from_str(holder), objective.to_owned());
        let current = self.scores.get(&key).copied().unwrap_or(0);
        self.scores.insert(key, current.wrapping_sub(amount));
    }

    pub fn reset(&mut self, holder: &str, objective: &str) {
        self.scores
            .remove(&(ScoreHolderKey::from_str(holder), objective.to_owned()));
    }

    pub fn reset_objective(&mut self, objective: &str) {
        self.scores.retain(|(_, obj), _| obj != objective);
    }

    pub fn scoreboard_operation(
        &mut self,
        target_holder: &str,
        target_obj: &str,
        op: ScoreOp,
        source_holder: &str,
        source_obj: &str,
    ) {
        let source = self
            .scores
            .get(&(
                ScoreHolderKey::from_str(source_holder),
                source_obj.to_owned(),
            ))
            .copied()
            .unwrap_or(0);
        let target = self
            .scores
            .get(&(
                ScoreHolderKey::from_str(target_holder),
                target_obj.to_owned(),
            ))
            .copied()
            .unwrap_or(0);
        let result = match op {
            ScoreOp::Assign => source,
            ScoreOp::Add => target.wrapping_add(source),
            ScoreOp::Subtract => target.wrapping_sub(source),
            ScoreOp::Multiply => target.wrapping_mul(source),
            ScoreOp::Divide => {
                if source == 0 {
                    // Division by zero leaves destination unchanged, success=0, result=0
                    return;
                }
                // Floor division (Minecraft uses floor, not truncation)
                floor_div(target, source)
            }
            ScoreOp::Modulo => {
                if source == 0 {
                    return;
                }
                // Remainder with divisor's sign
                target.wrapping_rem(source)
            }
            ScoreOp::Min => target.min(source),
            ScoreOp::Max => target.max(source),
            ScoreOp::Swap => {
                let tgt_key = (
                    ScoreHolderKey::from_str(target_holder),
                    target_obj.to_owned(),
                );
                let src_key = (
                    ScoreHolderKey::from_str(source_holder),
                    source_obj.to_owned(),
                );
                let target_val = self.scores.get(&tgt_key).copied().unwrap_or(0);
                let source_val = self.scores.get(&src_key).copied().unwrap_or(0);
                self.scores.insert(tgt_key, source_val);
                self.scores.insert(src_key, target_val);
                return;
            }
        };
        self.scores.insert(
            (
                ScoreHolderKey::from_str(target_holder),
                target_obj.to_owned(),
            ),
            result,
        );
    }

    /// Returns all scored holders that match the given prefix, with their scores
    /// for the given objective.
    pub fn holders_in_objective(&self, objective: &str, prefix: &str) -> Vec<(String, i32)> {
        let mut results = Vec::new();
        for ((holder_key, obj), score) in &self.scores {
            if obj == objective && holder_key.matches(prefix) {
                results.push((holder_key.to_string(), *score));
            }
        }
        results
    }
}

/// Floor division matching Java integer division semantics.
fn floor_div(a: i32, b: i32) -> i32 {
    let q = a / b;
    let r = a % b;
    if (r != 0) && ((a ^ b) < 0) { q - 1 } else { q }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScoreOp {
    Assign,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Min,
    Max,
    Swap,
}

impl ScoreOp {
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "=" => Some(Self::Assign),
            "+=" => Some(Self::Add),
            "-=" => Some(Self::Subtract),
            "*=" => Some(Self::Multiply),
            "/=" => Some(Self::Divide),
            "%=" => Some(Self::Modulo),
            "<" => Some(Self::Min),
            ">" => Some(Self::Max),
            "><" => Some(Self::Swap),
            _ => None,
        }
    }
}

/// A key for identifying score holders (fake players, entities, etc.)
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ScoreHolderKey {
    Wildcard,
    FakePlayer(String),
    Entity(String), // entity identifier
}

impl ScoreHolderKey {
    fn from_str(s: &str) -> Self {
        if s == "*" {
            Self::Wildcard
        } else if let Some(name) = s.strip_prefix('#') {
            Self::FakePlayer(name.to_owned())
        } else {
            Self::Entity(s.to_owned())
        }
    }

    fn matches(&self, pattern: &str) -> bool {
        match self {
            Self::Wildcard => true,
            Self::FakePlayer(name) => {
                if let Some(p) = pattern.strip_prefix('#') {
                    name == p
                } else {
                    false
                }
            }
            Self::Entity(_) => pattern == self.to_string(),
        }
    }
}

impl std::fmt::Display for ScoreHolderKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wildcard => write!(f, "*"),
            Self::FakePlayer(name) => write!(f, "#{name}"),
            Self::Entity(id) => write!(f, "{id}"),
        }
    }
}
