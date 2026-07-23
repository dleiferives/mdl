//! Closed compilation targets and their immutable facts.

mod java_26_2;

use std::fmt;

/// A supported Minecraft Java Edition target.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum JavaEditionTarget {
    /// Minecraft Java Edition 26.2.
    V26_2,
}

impl JavaEditionTarget {
    /// Returns the immutable facts for this target.
    #[must_use]
    pub const fn spec(self) -> &'static TargetSpec {
        TargetSpec::for_target(self)
    }
}

impl fmt::Display for JavaEditionTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.spec().game_version())
    }
}

/// Immutable facts that define one supported target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetSpec {
    game_version: &'static str,
    data_pack_format: [u32; 2],
    function_directory: &'static str,
    function_tag_directory: &'static str,
    advancement_directory: &'static str,
    default_max_command_sequence: u32,
    default_max_command_forks: u32,
    max_logical_command_utf16_units: u32,
    conformance_java_runtime: u16,
}

impl TargetSpec {
    /// Returns the immutable facts for `target`.
    #[must_use]
    pub const fn for_target(target: JavaEditionTarget) -> &'static Self {
        match target {
            JavaEditionTarget::V26_2 => &java_26_2::SPEC,
        }
    }

    /// Returns the Minecraft game version.
    #[must_use]
    pub const fn game_version(&self) -> &'static str {
        self.game_version
    }

    /// Returns the exact major/minor data-pack format.
    #[must_use]
    pub const fn data_pack_format(&self) -> [u32; 2] {
        self.data_pack_format
    }

    /// Returns the target-relative function resource directory.
    #[must_use]
    pub const fn function_directory(&self) -> &'static str {
        self.function_directory
    }

    /// Returns the target-relative function-tag resource directory.
    #[must_use]
    pub const fn function_tag_directory(&self) -> &'static str {
        self.function_tag_directory
    }

    /// Returns the target-relative advancement resource directory.
    #[must_use]
    pub const fn advancement_directory(&self) -> &'static str {
        self.advancement_directory
    }

    /// Returns the target's default maximum command sequence length.
    #[must_use]
    pub const fn default_max_command_sequence(&self) -> u32 {
        self.default_max_command_sequence
    }

    /// Returns the target's default maximum command fork count.
    #[must_use]
    pub const fn default_max_command_forks(&self) -> u32 {
        self.default_max_command_forks
    }

    /// Returns the maximum logical command length in Java UTF-16 code units.
    #[must_use]
    pub const fn max_logical_command_utf16_units(&self) -> u32 {
        self.max_logical_command_utf16_units
    }

    /// Returns the Java runtime major version used for conformance testing.
    #[must_use]
    pub const fn conformance_java_runtime(&self) -> u16 {
        self.conformance_java_runtime
    }
}

#[cfg(test)]
mod tests {
    use super::{JavaEditionTarget, TargetSpec};

    #[test]
    fn java_26_2_facts_are_exact() {
        let target = JavaEditionTarget::V26_2;
        let spec = target.spec();

        assert_eq!(spec, TargetSpec::for_target(target));
        assert_eq!(spec.game_version(), "26.2");
        assert_eq!(target.to_string(), "26.2");
        assert_eq!(spec.data_pack_format(), [107, 1]);
        assert_eq!(spec.function_directory(), "function");
        assert_eq!(spec.function_tag_directory(), "tags/function");
        assert_eq!(spec.advancement_directory(), "advancement");
        assert_eq!(spec.default_max_command_sequence(), 65_536);
        assert_eq!(spec.default_max_command_forks(), 65_536);
        assert_eq!(spec.max_logical_command_utf16_units(), 2_000_000);
        assert_eq!(spec.conformance_java_runtime(), 25);
    }
}
