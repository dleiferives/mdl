use std::collections::HashMap;

use super::nbt::NbtValue;

/// NBT storage engine: maps storage IDs to compound roots with path traversal.
#[derive(Debug, Clone)]
pub struct StorageEngine {
    storages: HashMap<String, NbtValue>,
}

impl StorageEngine {
    pub fn new() -> Self {
        Self { storages: HashMap::new() }
    }

    fn ensure_root(&mut self, storage_id: &str) {
        self.storages
            .entry(storage_id.to_owned())
            .or_insert_with(|| NbtValue::compound(vec![]));
    }

    // ── path parsing ─────────────────────────────────────────

    fn parse_nbt_path(path: &str) -> Result<Vec<NbtPathOp>, String> {
        let mut ops = Vec::new();
        let chars: Vec<char> = path.chars().collect();
        let mut pos = 0;

        while pos < chars.len() {
            match chars[pos] {
                '[' => {
                    pos += 1;
                    let mut index_str = String::new();
                    while pos < chars.len() && chars[pos] != ']' {
                        index_str.push(chars[pos]);
                        pos += 1;
                    }
                    if pos >= chars.len() {
                        return Err(format!("unterminated bracket in path: {path}"));
                    }
                    pos += 1;
                    if index_str.is_empty() {
                        ops.push(NbtPathOp::AllElements);
                    } else {
                        let idx: i32 = index_str.parse()
                            .map_err(|_| format!("invalid index: {index_str}"))?;
                        ops.push(NbtPathOp::Index(idx));
                    }
                }
                '.' => {
                    pos += 1;
                    let start = pos;
                    while pos < chars.len() && chars[pos] != '.' && chars[pos] != '[' {
                        pos += 1;
                    }
                    let key: String = chars[start..pos].iter().collect();
                    if key.is_empty() {
                        return Err(format!("empty key in path: {path}"));
                    }
                    ops.push(NbtPathOp::Key(key));
                }
                _ => {
                    let start = pos;
                    while pos < chars.len() && chars[pos] != '.' && chars[pos] != '[' {
                        pos += 1;
                    }
                    let key: String = chars[start..pos].iter().collect();
                    if key.is_empty() {
                        return Err(format!("empty first key in path: {path}"));
                    }
                    ops.push(NbtPathOp::Key(key));
                }
            }
        }
        if ops.is_empty() {
            return Err(format!("empty path"));
        }
        Ok(ops)
    }

    // ── read ──────────────────────────────────────────────────

    #[must_use]
    pub fn get(&self, storage_id: &str, path: &str) -> Option<NbtValue> {
        let ops = Self::parse_nbt_path(path).ok()?;
        self.resolve(storage_id, &ops).cloned()
    }

    #[must_use]
    pub fn exists(&self, storage_id: &str, path: &str) -> bool {
        self.get(storage_id, path).is_some()
    }

    fn resolve<'a>(&'a self, storage_id: &str, ops: &[NbtPathOp]) -> Option<&'a NbtValue> {
        let mut current = self.storages.get(storage_id)?;
        for op in ops {
            current = Self::step(current, op)?;
        }
        Some(current)
    }

    fn step<'a>(current: &'a NbtValue, op: &NbtPathOp) -> Option<&'a NbtValue> {
        match op {
            NbtPathOp::Key(key) => {
                let NbtValue::Compound(entries) = current else { return None; };
                entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
            }
            NbtPathOp::Index(idx) => {
                let NbtValue::List(values) = current else { return None; };
                let len = values.len() as i32;
                let i = if *idx < 0 { len + idx } else { *idx };
                if i < 0 || i >= len { return None; }
                Some(&values[i as usize])
            }
            NbtPathOp::AllElements => Some(current),
        }
    }

    // ── write ─────────────────────────────────────────────────

    pub fn set(&mut self, storage_id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let ops = Self::parse_nbt_path(path)?;
        self.ensure_root(storage_id);
        let root = self.storages.get_mut(storage_id).unwrap();
        Self::set_inner(root, &ops, 0, value)
    }

    fn set_inner(target: &mut NbtValue, ops: &[NbtPathOp], index: usize, value: NbtValue) -> Result<(), String> {
        if index >= ops.len() {
            *target = value;
            return Ok(());
        }
        let next_ops = &ops[index + 1..];
        let next_op = ops.get(index + 1);

        match &ops[index] {
            NbtPathOp::Key(key) => {
                let NbtValue::Compound(entries) = target else {
                    *target = NbtValue::compound(vec![]);
                    let NbtValue::Compound(entries) = target else { unreachable!() };
                    Self::insert_key(entries, key, next_op, next_ops, value)?;
                    return Ok(());
                };
                Self::insert_key(entries, key, next_op, next_ops, value)
            }
            NbtPathOp::Index(idx) => {
                let NbtValue::List(values) = target else {
                    return Err("cannot index non-list".to_owned());
                };
                let len = values.len() as i32;
                let i = if *idx < 0 { len + idx } else { *idx };
                if i < 0 || i >= len {
                    return Err(format!("index {idx} out of bounds"));
                }
                Self::set_inner(&mut values[i as usize], ops, index + 1, value)
            }
            NbtPathOp::AllElements => {
                let NbtValue::List(values) = target else {
                    return Err("cannot use [] on non-list".to_owned());
                };
                for val in values {
                    Self::set_inner(val, ops, index + 1, value.clone())?;
                }
                Ok(())
            }
        }
    }

    fn insert_key(
        entries: &mut Vec<(String, NbtValue)>,
        key: &str,
        next_op: Option<&NbtPathOp>,
        next_ops: &[NbtPathOp],
        value: NbtValue,
    ) -> Result<(), String> {
        if let Some(pos) = entries.iter().position(|(k, _)| k == key) {
            Self::set_inner(&mut entries[pos].1, next_ops, 0, value)
        } else {
            let intermediate = match next_op {
                Some(NbtPathOp::Index(_)) | Some(NbtPathOp::AllElements) => NbtValue::List(vec![]),
                _ => NbtValue::compound(vec![]),
            };
            entries.push((key.to_owned(), intermediate));
            let last = entries.last_mut().unwrap();
            Self::set_inner(&mut last.1, next_ops, 0, value)?;
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(())
        }
    }

    pub fn append(&mut self, storage_id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let ops = Self::parse_nbt_path(path)?;
        self.ensure_root(storage_id);
        let root = self.storages.get_mut(storage_id).unwrap();
        let list = Self::resolve_mut_list(root, &ops)
            .ok_or_else(|| format!("resolve failed: {path}"))?;
        list.push(value);
        Ok(())
    }

    pub fn prepend(&mut self, storage_id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let ops = Self::parse_nbt_path(path)?;
        self.ensure_root(storage_id);
        let root = self.storages.get_mut(storage_id).unwrap();
        let list = Self::resolve_mut_list(root, &ops)
            .ok_or_else(|| format!("resolve failed: {path}"))?;
        list.insert(0, value);
        Ok(())
    }

    pub fn merge(&mut self, storage_id: &str, path: &str, value: NbtValue) -> Result<(), String> {
        let ops = Self::parse_nbt_path(path)?;
        self.ensure_root(storage_id);
        let root = self.storages.get_mut(storage_id).unwrap();
        let compound = Self::resolve_mut_compound(root, &ops)
            .ok_or_else(|| format!("resolve failed: {path}"))?;
        let NbtValue::Compound(incoming) = value else {
            return Err("merge requires a compound value".to_owned());
        };
        for (key, val) in incoming {
            if let Some(pos) = compound.iter().position(|(k, _)| k == &key) {
                compound[pos].1 = val;
            } else {
                compound.push((key, val));
            }
        }
        compound.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(())
    }

    pub fn copy_from(
        &mut self,
        target_storage: &str,
        target_path: &str,
        source_storage: &str,
        source_path: &str,
    ) -> Result<(), String> {
        let source = self.get(source_storage, source_path)
            .ok_or_else(|| format!("source not found: {source_storage} {source_path}"))?;
        self.set(target_storage, target_path, source)
    }

    pub fn remove(&mut self, storage_id: &str, path: &str) -> Result<(), String> {
        let ops = Self::parse_nbt_path(path)?;
        if ops.is_empty() {
            self.storages.remove(storage_id);
            return Ok(());
        }
        let root = self.storages.get_mut(storage_id).ok_or("storage not found")?;
        Self::remove_inner(root, &ops, 0)
    }

    fn remove_inner(target: &mut NbtValue, ops: &[NbtPathOp], index: usize) -> Result<(), String> {
        if index >= ops.len() - 1 {
            match &ops[index] {
                NbtPathOp::Key(key) => {
                    let NbtValue::Compound(entries) = target else { return Err("not a compound".to_owned()); };
                    let pos = entries.iter().position(|(k, _)| k == key).ok_or_else(|| format!("key not found: {key}"))?;
                    entries.remove(pos);
                    Ok(())
                }
                NbtPathOp::Index(idx) => {
                    let NbtValue::List(values) = target else { return Err("not a list".to_owned()); };
                    let len = values.len() as i32;
                    let i = if *idx < 0 { len + idx } else { *idx };
                    if i < 0 || i >= len { return Err(format!("index out of bounds: {idx}")); }
                    values.remove(i as usize);
                    Ok(())
                }
                NbtPathOp::AllElements => Err("cannot remove with [] wildcard".to_owned()),
            }
        } else {
            match &ops[index] {
                NbtPathOp::Key(key) => {
                    let NbtValue::Compound(entries) = target else { return Err("not a compound".to_owned()); };
                    let pos = entries.iter().position(|(k, _)| k == key).ok_or_else(|| format!("key not found: {key}"))?;
                    Self::remove_inner(&mut entries[pos].1, ops, index + 1)
                }
                NbtPathOp::Index(idx) => {
                    let NbtValue::List(values) = target else { return Err("not a list".to_owned()); };
                    let len = values.len() as i32;
                    let i = if *idx < 0 { len + idx } else { *idx };
                    if i < 0 || i >= len { return Err(format!("index out of bounds: {idx}")); }
                    Self::remove_inner(&mut values[i as usize], ops, index + 1)
                }
                NbtPathOp::AllElements => {
                    let NbtValue::List(values) = target else { return Err("not a list".to_owned()); };
                    for val in values {
                        Self::remove_inner(val, ops, index + 1)?;
                    }
                    Ok(())
                }
            }
        }
    }

    // ── numeric store (for execute store result/success storage) ──

    pub fn store_numeric(
        &mut self,
        storage_id: &str,
        path: &str,
        numeric_type: &str,
        scale: f64,
        raw_value: i32,
    ) -> Result<(), String> {
        let scaled = (raw_value as f64) * scale;
        let value = match numeric_type {
            "byte" => NbtValue::Byte(scaled as i8),
            "short" => NbtValue::Short(scaled as i16),
            "int" => NbtValue::Int(scaled as i32),
            "long" => NbtValue::Long(scaled as i64),
            "float" => NbtValue::Float(scaled as f32),
            "double" => NbtValue::Double(scaled),
            _ => return Err(format!("unknown numeric type: {numeric_type}")),
        };
        self.set(storage_id, path, value)
    }

    #[must_use]
    pub fn get_root(&self, storage_id: &str) -> Option<&NbtValue> {
        self.storages.get(storage_id)
    }

    // ── internal helpers ──────────────────────────────────────

    fn resolve_mut_list<'a>(target: &'a mut NbtValue, ops: &[NbtPathOp]) -> Option<&'a mut Vec<NbtValue>> {
        let mut current: &mut NbtValue = target;
        for op in ops {
            current = match op {
                NbtPathOp::Key(key) => {
                    let NbtValue::Compound(entries) = current else { return None; };
                    let pos = entries.iter().position(|(k, _)| k == key)?;
                    &mut entries[pos].1
                }
                NbtPathOp::Index(idx) => {
                    let NbtValue::List(values) = current else { return None; };
                    let len = values.len() as i32;
                    let i = if *idx < 0 { len + idx } else { *idx };
                    if i < 0 || i >= len { return None; }
                    &mut values[i as usize]
                }
                NbtPathOp::AllElements => {
                    match current {
                        NbtValue::List(values) => return Some(values),
                        _ => return None,
                    }
                }
            };
        }
        match current {
            NbtValue::List(values) => Some(values),
            _ => None,
        }
    }

    fn resolve_mut_compound<'a>(target: &'a mut NbtValue, ops: &[NbtPathOp]) -> Option<&'a mut Vec<(String, NbtValue)>> {
        let mut current: &mut NbtValue = target;
        for op in ops {
            current = match op {
                NbtPathOp::Key(key) => {
                    let NbtValue::Compound(entries) = current else { return None; };
                    let pos = entries.iter().position(|(k, _)| k == key)?;
                    &mut entries[pos].1
                }
                NbtPathOp::Index(idx) => {
                    let NbtValue::List(values) = current else { return None; };
                    let len = values.len() as i32;
                    let i = if *idx < 0 { len + idx } else { *idx };
                    if i < 0 || i >= len { return None; }
                    &mut values[i as usize]
                }
                NbtPathOp::AllElements => return None,
            };
        }
        match current {
            NbtValue::Compound(entries) => Some(entries),
            _ => None,
        }
    }
}

impl Default for StorageEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq)]
enum NbtPathOp {
    Key(String),
    Index(i32),
    AllElements,
}
