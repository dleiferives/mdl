pub mod blocks;
pub mod entity;
pub mod nbt;
pub mod scoreboard;
pub mod storage;

use std::collections::HashMap;

use crate::version::MinecraftVersion;

pub use blocks::BlockMap;
pub use entity::EntityEngine;
pub use nbt::NbtValue;
pub use scoreboard::{ScoreOp, ScoreboardEngine};
pub use storage::StorageEngine;

/// The complete mutable state of the virtual Minecraft world.
#[derive(Debug, Clone)]
pub struct World {
    pub scoreboard: ScoreboardEngine,
    pub storage: StorageEngine,
    pub entities: EntityEngine,
    pub blocks: BlockMap,
    pub gamerules: GameruleState,
    pub tick: u64,
    pub version: MinecraftVersion,
}

impl World {
    #[must_use]
    pub fn new(version: MinecraftVersion) -> Self {
        let default_sequence = version.default_max_command_sequence_length;
        let default_forks = version.default_max_command_forks;
        Self {
            scoreboard: ScoreboardEngine::new(),
            storage: StorageEngine::new(),
            entities: EntityEngine::new(),
            blocks: BlockMap::new(),
            gamerules: GameruleState {
                rules: {
                    let mut rules = HashMap::new();
                    rules.insert(
                        "max_command_sequence_length".to_owned(),
                        i32::try_from(default_sequence).unwrap_or(i32::MAX),
                    );
                    rules.insert(
                        "max_command_forks".to_owned(),
                        i32::try_from(default_forks).unwrap_or(i32::MAX),
                    );
                    rules
                },
            },
            tick: 0,
            version,
        }
    }
}

/// Gamerule state.
#[derive(Debug, Clone, Default)]
pub struct GameruleState {
    pub rules: HashMap<String, i32>,
}

impl GameruleState {
    #[must_use]
    pub fn get(&self, name: &str) -> Option<i32> {
        self.rules.get(name).copied()
    }

    pub fn set(&mut self, name: &str, value: i32) {
        self.rules.insert(name.to_owned(), value);
    }
}
