use std::collections::HashMap;
use super::nbt::{NbtPathOp, NbtValue};

#[derive(Debug, Clone)]
pub struct StorageEngine { storages: HashMap<String, NbtValue> }

impl StorageEngine {
    pub fn new() -> Self { Self { storages: HashMap::new() } }
    fn ensure_root(&mut self, id: &str) { self.storages.entry(id.to_owned()).or_insert_with(|| NbtValue::compound(vec![])); }

    pub fn get(&self, id: &str, path: &str) -> Option<NbtValue> {
        let root = self.storages.get(id)?;
        let ops = NbtValue::parse_path(path).ok()?;
        root.resolve(&ops).cloned()
    }
    pub fn exists(&self, id: &str, path: &str) -> bool { self.get(id, path).is_some() }
    pub fn set(&mut self, id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        self.ensure_root(id);
        if path.is_empty() { *self.storages.get_mut(id).unwrap() = value; return Ok(()); }
        let ops = NbtValue::parse_path(path)?;
        self.storages.get_mut(id).unwrap().set_at(&ops, value)
    }
    pub fn append(&mut self, id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let ops = NbtValue::parse_path(path)?; self.ensure_root(id);
        self.storages.get_mut(id).unwrap().append_at(&ops, value)
    }
    pub fn prepend(&mut self, id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let ops = NbtValue::parse_path(path)?; self.ensure_root(id);
        self.storages.get_mut(id).unwrap().prepend_at(&ops, value)
    }
    pub fn merge(&mut self, id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        self.ensure_root(id);
        if path.is_empty() {
            let root = self.storages.get_mut(id).unwrap();
            let NbtValue::Compound(inc) = value else { return Err("merge needs compound".into()); };
            let NbtValue::Compound(entries) = root else { return Err("root not compound".into()); };
            for (k, v) in inc {
                if let Some(p) = entries.iter().position(|(key,_)| key == &k) { entries[p].1 = v; }
                else { entries.push((k, v)); }
            }
            entries.sort_by(|a,b| a.0.cmp(&b.0)); return Ok(());
        }
        let ops = NbtValue::parse_path(path)?;
        self.storages.get_mut(id).unwrap().merge_at(&ops, value)
    }
    pub fn copy_from(&mut self, target: &str, tpath: &str, source: &str, spath: &str) -> Result<(), String> {
        let src = self.get(source, spath).ok_or_else(|| format!("src not found: {source} {spath}"))?;
        self.set(target, tpath, src)
    }
    pub fn remove(&mut self, id: &str, path: &str) -> Result<(), String> {
        let ops = NbtValue::parse_path(path)?;
        self.storages.get_mut(id).ok_or("storage not found")?.remove_at(&ops)
    }
    pub fn store_numeric(&mut self, id: &str, path: &str, nt: &str, scale: f64, raw: i32) -> Result<(), String> {
        let scaled = (raw as f64) * scale;
        let val = match nt {
            "byte" => NbtValue::Byte(scaled as i8), "short" => NbtValue::Short(scaled as i16),
            "int" => NbtValue::Int(scaled as i32), "long" => NbtValue::Long(scaled as i64),
            "float" => NbtValue::Float(scaled as f32), "double" => NbtValue::Double(scaled),
            _ => return Err(format!("unknown numeric type: {nt}")),
        };
        self.set(id, path, val)
    }
    pub fn get_root(&self, id: &str) -> Option<&NbtValue> { self.storages.get(id) }
}

impl Default for StorageEngine { fn default() -> Self { Self::new() } }
