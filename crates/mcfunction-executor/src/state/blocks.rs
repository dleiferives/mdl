use std::collections::HashMap;
use super::nbt::NbtValue;

#[derive(Debug, Clone, Default)]
pub struct BlockMap { blocks: HashMap<BlockKey, BlockData> }

#[derive(Clone, Debug)]
struct BlockData { block: String, nbt: NbtValue }

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct BlockKey { x: i32, y: i32, z: i32, dimension: String }

impl BlockMap {
    pub fn new() -> Self { Self::default() }

    fn ensure(&mut self, x: i32, y: i32, z: i32, dim: &str) {
        let key = BlockKey { x, y, z, dimension: dim.to_owned() };
        self.blocks.entry(key).or_insert_with(|| BlockData { block: String::new(), nbt: NbtValue::compound(vec![]) });
    }

    pub fn set(&mut self, x: i32, y: i32, z: i32, dim: &str, block: &str) {
        let key = BlockKey { x, y, z, dimension: dim.to_owned() };
        match self.blocks.entry(key) {
            std::collections::hash_map::Entry::Occupied(mut e) => { e.get_mut().block = block.to_owned(); }
            std::collections::hash_map::Entry::Vacant(e) => { e.insert(BlockData { block: block.to_owned(), nbt: NbtValue::compound(vec![]) }); }
        }
    }

    pub fn get(&self, x: i32, y: i32, z: i32, dim: &str) -> Option<&str> {
        self.blocks.get(&BlockKey { x, y, z, dimension: dim.to_owned() }).map(|d| d.block.as_str())
    }

    pub fn matches(&self, x: i32, y: i32, z: i32, dim: &str, pred: &str) -> bool {
        self.get(x, y, z, dim) == Some(pred)
    }

    pub fn nbt_get(&self, x: i32, y: i32, z: i32, dim: &str, path: &str) -> Option<NbtValue> {
        let root = &self.blocks.get(&BlockKey { x, y, z, dimension: dim.to_owned() })?.nbt;
        let ops = NbtValue::parse_path(path).ok()?;
        root.resolve(&ops).cloned()
    }

    pub fn nbt_exists(&self, x: i32, y: i32, z: i32, dim: &str, path: &str) -> bool {
        self.nbt_get(x, y, z, dim, path).is_some()
    }

    pub fn nbt_set(&mut self, x: i32, y: i32, z: i32, dim: &str, path: &str, value: NbtValue) -> Result<(), String> {
        self.ensure(x, y, z, dim);
        let ops = NbtValue::parse_path(path)?;
        self.blocks.get_mut(&BlockKey { x, y, z, dimension: dim.to_owned() }).unwrap().nbt.set_at(&ops, value)
    }

    pub fn nbt_remove(&mut self, x: i32, y: i32, z: i32, dim: &str, path: &str) -> Result<(), String> {
        let ops = NbtValue::parse_path(path)?;
        self.blocks.get_mut(&BlockKey { x, y, z, dimension: dim.to_owned() }).ok_or("block not found")?.nbt.remove_at(&ops)
    }

    pub fn nbt_merge(&mut self, x: i32, y: i32, z: i32, dim: &str, path: &str, source: NbtValue) -> Result<(), String> {
        let ops = NbtValue::parse_path(path)?;
        self.blocks.get_mut(&BlockKey { x, y, z, dimension: dim.to_owned() }).ok_or("block not found")?.nbt.merge_at(&ops, source)
    }
}
