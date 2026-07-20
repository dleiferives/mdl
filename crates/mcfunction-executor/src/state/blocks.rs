use std::collections::HashMap;

/// Minimal block map — keyed by (x, y, z, dimension), sparse/empty by default.
/// Supports `setblock` writes and `execute if block` reads.
/// No block updates, no redstone, no physics.
#[derive(Debug, Clone, Default)]
pub struct BlockMap {
    blocks: HashMap<BlockKey, String>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct BlockKey {
    x: i32,
    y: i32,
    z: i32,
    dimension: String,
}

impl BlockMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, x: i32, y: i32, z: i32, dimension: &str, block: &str) {
        self.blocks.insert(
            BlockKey { x, y, z, dimension: dimension.to_owned() },
            block.to_owned(),
        );
    }

    #[must_use]
    pub fn get(&self, x: i32, y: i32, z: i32, dimension: &str) -> Option<&str> {
        self.blocks
            .get(&BlockKey { x, y, z, dimension: dimension.to_owned() })
            .map(String::as_str)
    }

    /// Check if the block at position matches a predicate.
    /// For now, just check exact block type match.
    #[must_use]
    pub fn matches(&self, x: i32, y: i32, z: i32, dimension: &str, predicate: &str) -> bool {
        self.get(x, y, z, dimension) == Some(predicate)
    }
}
