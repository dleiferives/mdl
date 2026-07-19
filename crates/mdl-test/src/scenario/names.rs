use std::error::Error;
use std::fmt;

/// Stable human-readable identity of one scenario.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScenarioId(Box<str>);

impl ScenarioId {
    /// Validates a slash-separated lowercase test identity.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty segment or a character outside
    /// `[a-z0-9_.-]`.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, NameError> {
        let value = value.into();
        validate_path(&value, "scenario ID")?;
        Ok(Self(value))
    }

    /// Returns the validated text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScenarioId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Safe single-component name owned by the test harness.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TestName(Box<str>);

impl TestName {
    /// Validates one nonempty lowercase name component.
    ///
    /// # Errors
    ///
    /// Returns an error for a slash or a character outside `[a-z0-9_.-]`.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, NameError> {
        let value = value.into();
        validate_component(&value, "test name")?;
        Ok(Self(value))
    }

    /// Returns the validated text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TestName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Validated `namespace:path` resource location.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResourceLocation {
    namespace: Box<str>,
    path: Box<str>,
}

impl ResourceLocation {
    /// Validates both resource-location components.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe namespace or path text.
    pub fn new(
        namespace: impl Into<Box<str>>,
        path: impl Into<Box<str>>,
    ) -> Result<Self, NameError> {
        let namespace = namespace.into();
        let path = path.into();
        validate_component(&namespace, "resource namespace")?;
        validate_path(&path, "resource path")?;
        Ok(Self { namespace, path })
    }

    /// Parses an explicit `namespace:path` resource location.
    ///
    /// # Errors
    ///
    /// Returns an error when the delimiter or either component is invalid.
    pub fn parse(value: &str) -> Result<Self, NameError> {
        let Some((namespace, path)) = value.split_once(':') else {
            return Err(NameError::new(
                "resource location",
                value,
                "expected an explicit namespace:path",
            ));
        };
        if path.contains(':') {
            return Err(NameError::new(
                "resource location",
                value,
                "contains more than one ':' delimiter",
            ));
        }
        Self::new(namespace, path)
    }

    /// Returns the namespace component.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Returns the path component.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for ResourceLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.namespace, self.path)
    }
}

/// Nonzero identity separating repeated executions of one scenario.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GenerationNonce(u64);

impl GenerationNonce {
    /// Creates a nonzero generation nonce.
    ///
    /// # Errors
    ///
    /// Returns an error because zero is reserved for uninitialized world state.
    pub const fn new(value: u64) -> Result<Self, NameError> {
        if value == 0 {
            return Err(NameError {
                kind: "generation nonce",
                value: String::new(),
                reason: "zero is reserved",
            });
        }
        Ok(Self(value))
    }

    /// Returns the numeric nonce.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Validation failure for a test-owned name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameError {
    kind: &'static str,
    value: String,
    reason: &'static str,
}

impl NameError {
    fn new(kind: &'static str, value: &str, reason: &'static str) -> Self {
        Self {
            kind,
            value: value.to_owned(),
            reason,
        }
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.value.is_empty() {
            write!(formatter, "invalid {}: {}", self.kind, self.reason)
        } else {
            write!(
                formatter,
                "invalid {} {:?}: {}",
                self.kind, self.value, self.reason
            )
        }
    }
}

impl Error for NameError {}

fn validate_path(value: &str, kind: &'static str) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::new(kind, value, "must not be empty"));
    }
    for segment in value.split('/') {
        validate_component(segment, kind)?;
    }
    Ok(())
}

fn validate_component(value: &str, kind: &'static str) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::new(kind, value, "must not be empty"));
    }
    if matches!(value, "." | "..") {
        return Err(NameError::new(kind, value, "must not be '.' or '..'"));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte))
    {
        return Err(NameError::new(
            kind,
            value,
            "expected only lowercase ASCII letters, digits, '_', '.', or '-'",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GenerationNonce, ResourceLocation, ScenarioId, TestName};

    #[test]
    fn names_reject_command_and_path_injection() {
        for bad in ["", "UPPER", "space here", "line\nbreak", "a//b", "../x"] {
            assert!(ScenarioId::new(bad).is_err(), "accepted {bad:?}");
        }
        assert!(TestName::new("has/slash").is_err());
        assert!(ResourceLocation::parse("missing_namespace").is_err());
        assert!(ResourceLocation::parse("mdl:path:again").is_err());
        assert!(GenerationNonce::new(0).is_err());
    }

    #[test]
    fn stable_names_accept_the_supported_surface() {
        assert_eq!(
            ScenarioId::new("scalar/call-baseline").unwrap().as_str(),
            "scalar/call-baseline"
        );
        assert_eq!(TestName::new("mdl.test-1").unwrap().as_str(), "mdl.test-1");
        assert_eq!(
            ResourceLocation::parse("mdl_test:result/scalar.call")
                .unwrap()
                .to_string(),
            "mdl_test:result/scalar.call"
        );
    }
}
