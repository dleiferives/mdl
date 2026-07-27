use super::nbt::NbtValue;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct StorageEngine {
    storages: HashMap<String, NbtValue>,
}

impl StorageEngine {
    pub fn new() -> Self {
        Self {
            storages: HashMap::new(),
        }
    }
    fn root_mut(&mut self, id: &str) -> &mut NbtValue {
        self.storages
            .entry(id.to_owned())
            .or_insert_with(|| NbtValue::compound(vec![]))
    }

    pub fn get(&self, id: &str, path: &str) -> Option<NbtValue> {
        let root = self.storages.get(id)?;
        if path.is_empty() {
            return Some(root.clone());
        }
        let ops = NbtValue::parse_path(path).ok()?;
        root.resolve(&ops).cloned()
    }
    pub fn exists(&self, id: &str, path: &str) -> bool {
        self.get(id, path).is_some()
    }
    pub fn set(&mut self, id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let root = self.root_mut(id);
        if path.is_empty() {
            *root = value;
            return Ok(());
        }
        let ops = NbtValue::parse_path(path)?;
        root.set_at(&ops, value)
    }
    pub fn append(&mut self, id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let ops = NbtValue::parse_path(path)?;
        self.root_mut(id).append_at(&ops, value)
    }
    pub fn prepend(&mut self, id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let ops = NbtValue::parse_path(path)?;
        self.root_mut(id).prepend_at(&ops, value)
    }
    pub fn merge(&mut self, id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let root = self.root_mut(id);
        if path.is_empty() {
            let NbtValue::Compound(inc) = value else {
                return Err("merge needs compound".into());
            };
            let NbtValue::Compound(entries) = root else {
                return Err("root not compound".into());
            };
            for (k, v) in inc {
                if let Some(p) = entries.iter().position(|(key, _)| key == &k) {
                    entries[p].1 = v;
                } else {
                    entries.push((k, v));
                }
            }
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            return Ok(());
        }
        let ops = NbtValue::parse_path(path)?;
        root.merge_at(&ops, value)
    }
    pub fn copy_from(
        &mut self,
        target: &str,
        tpath: &str,
        source: &str,
        spath: &str,
    ) -> Result<(), String> {
        let src = self
            .get(source, spath)
            .ok_or_else(|| format!("src not found: {source} {spath}"))?;
        self.set(target, tpath, src)
    }
    pub fn remove(&mut self, id: &str, path: &str) -> Result<(), String> {
        let ops = NbtValue::parse_path(path)?;
        self.storages
            .get_mut(id)
            .ok_or("storage not found")?
            .remove_at(&ops)
    }
    pub fn store_numeric(
        &mut self,
        id: &str,
        path: &str,
        nt: &str,
        scale: f64,
        raw: i32,
    ) -> Result<(), String> {
        let val = NbtValue::from_scaled_i32(raw, nt, scale)?;
        self.set(id, path, val)
    }
    pub fn get_root(&self, id: &str) -> Option<&NbtValue> {
        self.storages.get(id)
    }
}

impl Default for StorageEngine {
    fn default() -> Self {
        Self::new()
    }
}
