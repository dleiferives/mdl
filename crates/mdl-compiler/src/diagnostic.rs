//! Shared compiler diagnostics.

use std::error::Error;
use std::fmt;

use crate::source::OriginId;

/// One independently actionable compiler finding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    code: &'static str,
    message: String,
    origin: OriginId,
}

impl Diagnostic {
    pub(crate) fn new(code: &'static str, message: impl Into<String>, origin: OriginId) -> Self {
        Self {
            code,
            message: message.into(),
            origin,
        }
    }

    /// Returns the stable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the human-readable diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the best available provenance for the finding.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {} ({:?})",
            self.code, self.message, self.origin
        )
    }
}

/// Accumulated compiler diagnostics in deterministic production order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostics {
    findings: Vec<Diagnostic>,
}

impl Diagnostics {
    pub(crate) fn from_findings(findings: Vec<Diagnostic>) -> Option<Self> {
        (!findings.is_empty()).then_some(Self { findings })
    }

    pub(crate) fn into_findings(self) -> Vec<Diagnostic> {
        self.findings
    }

    /// Returns all findings in deterministic production order.
    #[must_use]
    pub fn findings(&self) -> &[Diagnostic] {
        &self.findings
    }

    /// Returns the number of findings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// Returns whether no findings are present.
    ///
    /// A constructed [`Diagnostics`] value is always nonempty. This method remains
    /// useful to callers inspecting a borrowed value and preserves the original
    /// Core verifier API.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    /// Returns whether at least one finding has the given stable code.
    #[must_use]
    pub fn contains_code(&self, code: &str) -> bool {
        self.findings.iter().any(|finding| finding.code == code)
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, finding) in self.findings.iter().enumerate() {
            if index != 0 {
                formatter.write_str("\n")?;
            }
            write!(formatter, "{finding}")?;
        }
        Ok(())
    }
}

impl Error for Diagnostics {}

#[cfg(test)]
mod tests {
    use super::Diagnostics;

    #[test]
    fn core_reexports_the_shared_diagnostics_type() {
        fn accepts_shared(_: Option<Diagnostics>) {}

        let through_core: Option<crate::ir::core::Diagnostics> = None;
        accepts_shared(through_core);
    }
}
