use std::fmt;

/// Version-specific Minecraft behavior configuration.
#[derive(Clone, Debug)]
pub struct MinecraftVersion {
    /// Data pack format version (e.g., [107, 1] for 26.2).
    pub data_pack_format: [u32; 2],
    /// Default `max_command_sequence_length` gamerule value.
    pub default_max_command_sequence_length: u32,
    /// Default `max_command_forks` gamerule value.
    pub default_max_command_forks: u32,
    /// Maximum function call recursion depth.
    pub max_recursion_depth: u32,
}

/// Minecraft Java Edition 26.2 behavior configuration.
pub const V26_2: MinecraftVersion = MinecraftVersion {
    data_pack_format: [107, 1],
    default_max_command_sequence_length: 65_536,
    default_max_command_forks: 65_536,
    max_recursion_depth: 256,
};

impl MinecraftVersion {
    /// Returns the human-readable label for this version.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self.data_pack_format {
            [107, 1] => "26.2",
            _ => "unknown",
        }
    }
}

impl fmt::Display for MinecraftVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl Default for MinecraftVersion {
    fn default() -> Self {
        V26_2
    }
}
