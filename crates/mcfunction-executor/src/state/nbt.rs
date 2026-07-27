use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum NbtValue {
    Byte(i8), Short(i16), Int(i32), Long(i64),
    Float(f32), Double(f64), String(String),
    List(Vec<NbtValue>), Compound(Vec<(String, NbtValue)>),
}

impl NbtValue {
    pub fn as_ref(&self) -> NbtValueRef<'_> {
        match self {
            Self::Byte(v) => NbtValueRef::Byte(*v), Self::Short(v) => NbtValueRef::Short(*v),
            Self::Int(v) => NbtValueRef::Int(*v), Self::Long(v) => NbtValueRef::Long(*v),
            Self::Float(v) => NbtValueRef::Float(*v), Self::Double(v) => NbtValueRef::Double(*v),
            Self::String(v) => NbtValueRef::String(v), Self::List(v) => NbtValueRef::List(v),
            Self::Compound(v) => NbtValueRef::Compound(v),
        }
    }
    pub fn is_compound(&self) -> bool { matches!(self, Self::Compound(_)) }
    pub fn is_list(&self) -> bool { matches!(self, Self::List(_)) }
    #[must_use] pub fn as_i32(&self) -> i32 {
        match self { Self::Byte(v) => i32::from(*v), Self::Short(v) => i32::from(*v), Self::Int(v) => *v, Self::Long(v) => *v as i32, Self::Float(v) => *v as i32, Self::Double(v) => *v as i32, Self::String(v) => v.len() as i32, Self::List(v) => v.len() as i32, Self::Compound(_) => 1 }
    }
    #[must_use] pub fn as_f64(&self) -> f64 {
        match self { Self::Byte(v) => f64::from(*v), Self::Short(v) => f64::from(*v), Self::Int(v) => f64::from(*v), Self::Long(v) => *v as f64, Self::Float(v) => f64::from(*v), Self::Double(v) => *v, _ => 0.0 }
    }
    #[must_use] pub fn as_f32(&self) -> f32 {
        match self { Self::Byte(v) => f32::from(*v), Self::Short(v) => f32::from(*v), Self::Int(v) => *v as f32, Self::Long(v) => *v as f32, Self::Float(v) => *v, Self::Double(v) => *v as f32, _ => 0.0 }
    }

    pub fn compound(entries: Vec<(String, NbtValue)>) -> Self {
        let mut sorted = entries; sorted.sort_by(|a,b| a.0.cmp(&b.0)); Self::Compound(sorted)
    }

    pub fn to_snbt(&self) -> String {
        match self {
            Self::Byte(v) => format!("{v}b"), Self::Short(v) => format!("{v}s"), Self::Int(v) => format!("{v}"), Self::Long(v) => format!("{v}L"),
            Self::Float(v) => if *v == -0.0 { "0.0f".into() } else { format!("{v}f") },
            Self::Double(v) => if *v == -0.0 { "0.0d".into() } else { format!("{v}d") },
            Self::String(v) => {
                let e: String = v.chars().map(|c| match c { '"' => "\\\"".to_owned(), '\\' => "\\\\".to_owned(), '\n' => "\\n".to_owned(), '\r' => "\\r".to_owned(), '\t' => "\\t".to_owned(), c => c.to_string() }).collect();
                format!("\"{e}\"")
            }
            Self::List(vals) => format!("[{}]", vals.iter().map(Self::to_snbt).collect::<Vec<_>>().join(",")),
            Self::Compound(entries) => format!("{{{}}}", entries.iter().map(|(k,v)| format!("{k}:{}", v.to_snbt())).collect::<Vec<_>>().join(",")),
        }
    }

    pub fn from_snbt(snbt: &str) -> Result<Self, String> {
        let s = snbt.trim();
        if s.starts_with('{') { return parse_compound(s); }
        if s.starts_with('[') { return parse_list(s); }
        if s.starts_with('"') { return parse_str(s); }
        if let Some(r) = s.strip_suffix('b').or_else(|| s.strip_suffix('B')) { return Ok(Self::Byte(r.parse().map_err(|_| format!("bad byte: {r}"))?)); }
        if let Some(r) = s.strip_suffix('s').or_else(|| s.strip_suffix('S')) { return Ok(Self::Short(r.parse().map_err(|_| format!("bad short: {r}"))?)); }
        if let Some(r) = s.strip_suffix('l').or_else(|| s.strip_suffix('L')) { return Ok(Self::Long(r.parse().map_err(|_| format!("bad long: {r}"))?)); }
        if let Some(r) = s.strip_suffix('f').or_else(|| s.strip_suffix('F')) { return Ok(Self::Float(r.parse().map_err(|_| format!("bad float: {r}"))?)); }
        if let Some(r) = s.strip_suffix('d').or_else(|| s.strip_suffix('D')) { return Ok(Self::Double(r.parse().map_err(|_| format!("bad double: {r}"))?)); }
        Ok(Self::Int(s.parse().map_err(|_| format!("bad int: {s}"))?))
    }

    pub fn parse_path(path: &str) -> Result<Vec<NbtPathOp>, String> {
        let mut ops = Vec::new(); let chars: Vec<char> = path.chars().collect(); let mut pos = 0;
        while pos < chars.len() {
            match chars[pos] {
                '[' => { pos += 1; let mut idx = String::new();
                    while pos < chars.len() && chars[pos] != ']' { idx.push(chars[pos]); pos += 1; }
                    if pos >= chars.len() { return Err("unterminated [".into()); }
                    pos += 1;
                    if idx.is_empty() { ops.push(NbtPathOp::AllElements); }
                    else { ops.push(NbtPathOp::Index(idx.parse().map_err(|_| format!("bad idx: {idx}"))?)); }
                }
                '.' => { pos += 1; let s = pos;
                    while pos < chars.len() && chars[pos] != '.' && chars[pos] != '[' { pos += 1; }
                    let k: String = chars[s..pos].iter().collect();
                    if k.is_empty() { return Err("empty key".into()); }
                    ops.push(NbtPathOp::Key(k));
                }
                _ => { let s = pos;
                    while pos < chars.len() && chars[pos] != '.' && chars[pos] != '[' { pos += 1; }
                    let k: String = chars[s..pos].iter().collect();
                    if k.is_empty() { return Err("empty key".into()); }
                    ops.push(NbtPathOp::Key(k));
                }
            }
        }
        if ops.is_empty() { Err("empty path".into()) } else { Ok(ops) }
    }

    pub fn resolve(&self, ops: &[NbtPathOp]) -> Option<&NbtValue> {
        let mut cur = self; for op in ops { cur = step(cur, op)?; } Some(cur)
    }

    pub fn set_at(&mut self, ops: &[NbtPathOp], value: NbtValue) -> Result<(), String> { set_inner(self, ops, 0, value) }
    pub fn remove_at(&mut self, ops: &[NbtPathOp]) -> Result<(), String> { remove_inner(self, ops, 0) }
    pub fn merge_at(&mut self, ops: &[NbtPathOp], source: NbtValue) -> Result<(), String> {
        let NbtValue::Compound(incoming) = source else { return Err("merge needs compound".into()); };
        let compound: &mut Vec<(String, NbtValue)> = resolve_mut_compound(self, ops).ok_or("resolve failed".to_owned())?;
        for (k, v) in incoming {
            if let Some(p) = compound.iter().position(|(key,_)| key == &k) { compound[p].1 = v; }
            else { compound.push((k, v)); }
        }
        compound.sort_by(|a,b| a.0.cmp(&b.0)); Ok(())
    }
    pub fn append_at(&mut self, ops: &[NbtPathOp], value: NbtValue) -> Result<(), String> {
        let list: &mut Vec<NbtValue> = resolve_mut_list(self, ops).ok_or("resolve failed".to_owned())?; list.push(value); Ok(())
    }
    pub fn prepend_at(&mut self, ops: &[NbtPathOp], value: NbtValue) -> Result<(), String> {
        let list: &mut Vec<NbtValue> = resolve_mut_list(self, ops).ok_or("resolve failed".to_owned())?; list.insert(0, value); Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NbtPathOp { Key(String), Index(i32), AllElements }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NbtValueRef<'a> {
    Byte(i8), Short(i16), Int(i32), Long(i64), Float(f32), Double(f64), String(&'a str), List(&'a [NbtValue]), Compound(&'a [(String, NbtValue)]),
}

impl fmt::Display for NbtValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.to_snbt()) }
}

// ── path internals ──

fn step<'a>(cur: &'a NbtValue, op: &NbtPathOp) -> Option<&'a NbtValue> {
    match op {
        NbtPathOp::Key(k) => { let NbtValue::Compound(e) = cur else { return None }; e.iter().find(|(key,_)| key == k).map(|(_,v)| v) }
        NbtPathOp::Index(i) => { let NbtValue::List(v) = cur else { return None }; let len = v.len() as i32; let idx = if *i < 0 { len + i } else { *i }; if idx < 0 || idx >= len { None } else { Some(&v[idx as usize]) } }
        NbtPathOp::AllElements => Some(cur),
    }
}

fn set_inner(target: &mut NbtValue, ops: &[NbtPathOp], index: usize, value: NbtValue) -> Result<(), String> {
    if index >= ops.len() { *target = value; return Ok(()); }
    let next = &ops[index+1..]; let next_op = ops.get(index+1);
    match &ops[index] {
        NbtPathOp::Key(key) => {
            let NbtValue::Compound(entries) = target else { *target = NbtValue::compound(vec![]); let NbtValue::Compound(entries) = target else { unreachable!() }; insert_key(entries, key, next_op, next, value)?; return Ok(()); };
            insert_key(entries, key, next_op, next, value)
        }
        NbtPathOp::Index(i) => {
            let NbtValue::List(vals) = target else { return Err("cannot index non-list".into()); };
            let len = vals.len() as i32; let idx = if *i < 0 { len + i } else { *i };
            if idx < 0 || idx >= len { return Err(format!("OOB: {i}")); }
            set_inner(&mut vals[idx as usize], ops, index+1, value)
        }
        NbtPathOp::AllElements => {
            let NbtValue::List(vals) = target else { return Err("[] on non-list".into()); };
            for v in vals { set_inner(v, ops, index+1, value.clone())?; } Ok(())
        }
    }
}

fn insert_key(e: &mut Vec<(String, NbtValue)>, key: &str, next_op: Option<&NbtPathOp>, next_ops: &[NbtPathOp], value: NbtValue) -> Result<(), String> {
    if let Some(p) = e.iter().position(|(k,_)| k == key) { return set_inner(&mut e[p].1, next_ops, 0, value); }
    let mid = match next_op { Some(NbtPathOp::Index(_)) | Some(NbtPathOp::AllElements) => NbtValue::List(vec![]), _ => NbtValue::compound(vec![]) };
    e.push((key.to_owned(), mid)); set_inner(&mut e.last_mut().unwrap().1, next_ops, 0, value)?; e.sort_by(|a,b| a.0.cmp(&b.0)); Ok(())
}

fn remove_inner(target: &mut NbtValue, ops: &[NbtPathOp], index: usize) -> Result<(), String> {
    if index >= ops.len()-1 {
        match &ops[index] {
            NbtPathOp::Key(k) => { let NbtValue::Compound(e) = target else { return Err("not compound".into()); }; let p = e.iter().position(|(key,_)| key == k).ok_or_else(|| format!("key not found: {k}"))?; e.remove(p); Ok(()) }
            NbtPathOp::Index(i) => { let NbtValue::List(v) = target else { return Err("not list".into()); }; let len = v.len() as i32; let idx = if *i < 0 { len + i } else { *i }; if idx < 0 || idx >= len { return Err(format!("OOB: {i}")); } v.remove(idx as usize); Ok(()) }
            NbtPathOp::AllElements => Err("cannot remove []".into()),
        }
    } else {
        match &ops[index] {
            NbtPathOp::Key(k) => { let NbtValue::Compound(e) = target else { return Err("not compound".into()); }; let p = e.iter().position(|(key,_)| key == k).ok_or_else(|| format!("key not found: {k}"))?; remove_inner(&mut e[p].1, ops, index+1) }
            NbtPathOp::Index(i) => { let NbtValue::List(v) = target else { return Err("not list".into()); }; let len = v.len() as i32; let idx = if *i < 0 { len + i } else { *i }; if idx < 0 || idx >= len { return Err(format!("OOB: {i}")); } remove_inner(&mut v[idx as usize], ops, index+1) }
            NbtPathOp::AllElements => { let NbtValue::List(v) = target else { return Err("not list".into()); }; for val in v { remove_inner(val, ops, index+1)?; } Ok(()) }
        }
    }
}

fn resolve_mut_list<'a>(target: &'a mut NbtValue, ops: &[NbtPathOp]) -> Option<&'a mut Vec<NbtValue>> {
    let mut cur: &mut NbtValue = target;
    for op in ops { cur = match op { NbtPathOp::Key(k) => { let NbtValue::Compound(e) = cur else { return None }; let p = e.iter().position(|(key,_)| key == k)?; &mut e[p].1 } NbtPathOp::Index(i) => { let NbtValue::List(v) = cur else { return None }; let len = v.len() as i32; let idx = if *i < 0 { len + i } else { *i }; if idx < 0 || idx >= len { return None } &mut v[idx as usize] } NbtPathOp::AllElements => match cur { NbtValue::List(v) => return Some(v), _ => return None } }; }
    match cur { NbtValue::List(v) => Some(v), _ => None }
}

fn resolve_mut_compound<'a>(target: &'a mut NbtValue, ops: &[NbtPathOp]) -> Option<&'a mut Vec<(String, NbtValue)>> {
    let mut cur: &mut NbtValue = target;
    for op in ops { cur = match op { NbtPathOp::Key(k) => { let NbtValue::Compound(e) = cur else { return None }; let p = e.iter().position(|(key,_)| key == k)?; &mut e[p].1 } NbtPathOp::Index(i) => { let NbtValue::List(v) = cur else { return None }; let len = v.len() as i32; let idx = if *i < 0 { len + i } else { *i }; if idx < 0 || idx >= len { return None } &mut v[idx as usize] } NbtPathOp::AllElements => return None }; }
    match cur { NbtValue::Compound(e) => Some(e), _ => None }
}

// ── SNBT parsing ──

fn parse_str(s: &str) -> Result<NbtValue, String> {
    let s = s.trim(); let u = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')).ok_or_else(|| format!("bad str: {s}"))?;
    let mut r = String::new(); let mut ch = u.chars();
    while let Some(c) = ch.next() {
        if c == '\\' { match ch.next() { Some('"') => r.push('"'), Some('\\') => r.push('\\'), Some('n') => r.push('\n'), Some('t') => r.push('\t'), Some('r') => r.push('\r'), Some(c) => r.push(c), None => break } }
        else { r.push(c); }
    }
    Ok(NbtValue::String(r))
}

fn parse_list(s: &str) -> Result<NbtValue, String> {
    let s = s.trim(); let inner = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')).ok_or_else(|| format!("bad list: {s}"))?;
    if inner.is_empty() { return Ok(NbtValue::List(vec![])); }
    let els = split_snbt(inner);
    let mut vals: Vec<NbtValue> = Vec::new();
    for e in els { vals.push(NbtValue::from_snbt(&e)?); }
    Ok(NbtValue::List(vals))
}

fn parse_compound(s: &str) -> Result<NbtValue, String> {
    let s = s.trim(); let inner = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')).ok_or_else(|| format!("bad compound: {s}"))?;
    if inner.is_empty() { return Ok(NbtValue::compound(vec![])); }
    let pairs = split_snbt(inner); let mut entries = Vec::new();
    for pair in pairs {
        let colon = pair.find(':').ok_or_else(|| format!("missing colon: {pair}"))?;
        let key = pair[..colon].trim(); let val = pair[colon+1..].trim();
        let key = key.strip_prefix('"').and_then(|k| k.strip_suffix('"')).unwrap_or(key);
        let parsed = NbtValue::from_snbt(val)?;
        entries.push((key.to_owned(), parsed));
    }
    Ok(NbtValue::compound(entries))
}

fn split_snbt(s: &str) -> Vec<String> {
    let mut els = Vec::new(); let mut cur = String::new(); let mut depth: i32 = 0; let mut in_str = false;
    for c in s.chars() {
        match c {
            '"' => { in_str = !in_str; cur.push(c); }
            '{' | '[' if !in_str => { depth += 1; cur.push(c); }
            '}' | ']' if !in_str => { depth -= 1; cur.push(c); }
            ',' if !in_str && depth == 0 => { els.push(std::mem::take(&mut cur)); }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { els.push(cur); }
    els
}
