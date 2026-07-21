use crate::state::nbt::{NbtPathOp, NbtValue};
use crate::state::scoreboard::ScoreboardEngine;

#[derive(Debug, Clone)]
pub struct EntityEngine { entities: Vec<Entity>, next_id: u64 }

#[derive(Debug, Clone)]
pub struct Entity {
    pub id: u64, pub entity_type: String, pub tags: Vec<String>,
    pub dimension: String, pub x: f64, pub y: f64, pub z: f64,
    pub yaw: f32, pub pitch: f32, pub nbt: NbtValue,
    pub gamemode: Option<String>, pub advancements: Vec<String>,
}
impl Entity {
    pub fn is_player(&self) -> bool { self.entity_type == "minecraft:player" }
    pub fn gamemode_str(&self) -> &str { self.gamemode.as_deref().unwrap_or("survival") }
}

#[derive(Clone, Debug, Default)]
struct SelectorFilters {
    type_filter: Option<String>, tag_filter: Option<String>, limit: Option<usize>,
    distance_min: Option<f64>, distance_max: Option<f64>,
    ref_x: Option<f64>, ref_y: Option<f64>, ref_z: Option<f64>,
    dx: Option<f64>, dy: Option<f64>, dz: Option<f64>,
    yaw_min: Option<f32>, yaw_max: Option<f32>, pitch_min: Option<f32>, pitch_max: Option<f32>,
    sort: SortOrder, gamemode: Option<String>, gamemode_negated: bool,
    nbt_filter: Option<NbtValue>, scores_filter: Vec<(String, String)>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum SortOrder { #[default] Arbitrary, Nearest, Furthest }

impl EntityEngine {
    pub fn new() -> Self { Self { entities: vec![], next_id: 1 } }

    pub fn summon(&mut self, entity_type: &str, x: f64, y: f64, z: f64, tags: &[String]) -> u64 {
        self.summon_with_nbt(entity_type, x, y, z, tags, NbtValue::compound(vec![]))
    }
    pub fn summon_with_nbt(&mut self, entity_type: &str, x: f64, y: f64, z: f64, tags: &[String], nbt: NbtValue) -> u64 {
        let id = self.next_id; self.next_id += 1;
        self.entities.push(Entity { id, entity_type: entity_type.to_owned(), tags: tags.to_vec(), dimension: "minecraft:overworld".to_owned(), x, y, z, yaw: 0.0, pitch: 0.0, nbt, gamemode: None, advancements: vec![] });
        id
    }
    pub fn spawn_player(&mut self, x: f64, y: f64, z: f64, gm: &str, tags: &[String]) -> u64 {
        let id = self.next_id; self.next_id += 1;
        self.entities.push(Entity { id, entity_type: "minecraft:player".to_owned(), tags: tags.to_vec(), dimension: "minecraft:overworld".to_owned(), x, y, z, yaw: 0.0, pitch: 0.0, nbt: NbtValue::compound(vec![]), gamemode: Some(gm.to_owned()), advancements: vec![] });
        id
    }
    pub fn grant_advancement(&mut self, eid: u64, adv: &str) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == eid) { if !e.advancements.contains(&adv.to_owned()) { e.advancements.push(adv.to_owned()); } }
    }
    pub fn revoke_advancement(&mut self, eid: u64, adv: &str) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == eid) { e.advancements.retain(|a| a != adv); }
    }
    pub fn has_advancement(&self, eid: u64, adv: &str) -> bool {
        self.entities.iter().find(|e| e.id == eid).map_or(false, |e| e.advancements.contains(&adv.to_owned()))
    }
    pub fn add_tag(&mut self, eid: u64, tag: &str) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == eid) { if !e.tags.contains(&tag.to_owned()) { e.tags.push(tag.to_owned()); } }
    }
    pub fn remove_tag(&mut self, eid: u64, tag: &str) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == eid) { e.tags.retain(|t| t != tag); }
    }
    pub fn remove_entity(&mut self, eid: u64) { self.entities.retain(|e| e.id != eid); }
    pub fn remove_all_matching(&mut self, sel: &str, sb: &ScoreboardEngine) -> usize {
        let ids = self.resolve_selector(sel, sb); let c = ids.len();
        for id in ids { self.entities.retain(|e| e.id != id); } c
    }

    pub fn teleport_absolute(&mut self, eid: u64, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == eid) { e.x = x; e.y = y; e.z = z; e.yaw = yaw; e.pitch = pitch; }
    }
    pub fn teleport_relative(&mut self, eid: u64, dx: f64, dy: f64, dz: f64) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == eid) { e.x += dx; e.y += dy; e.z += dz; }
    }
    pub fn teleport_local(&mut self, eid: u64, left: f64, up: f64, fwd: f64, eyes: bool) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == eid) {
            let y = e.yaw as f64; let yr = y.to_radians(); let c = yr.cos(); let s = yr.sin();
            let ey = if eyes { 1.27 } else { 0.0 };
            e.x += fwd * s - left * c; e.y += up + ey; e.z += fwd * c + left * s;
        }
    }
    pub fn set_rotation(&mut self, eid: u64, yaw: f32, pitch: f32) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == eid) { e.yaw = yaw; e.pitch = pitch; }
    }
    pub fn set_dimension(&mut self, eid: u64, dim: &str) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == eid) { e.dimension = dim.to_owned(); }
    }
    pub fn get(&self, id: u64) -> Option<&Entity> { self.entities.iter().find(|e| e.id == id) }
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Entity> { self.entities.iter_mut().find(|e| e.id == id) }

    // ── selector resolution ──

    pub fn resolve_selector(&self, sel: &str, sb: &ScoreboardEngine) -> Vec<u64> { self.resolve_selector_at(sel, 0.0, 0.0, 0.0, sb) }
    pub fn resolve_selector_at(&self, sel: &str, rx: f64, ry: f64, rz: f64, sb: &ScoreboardEngine) -> Vec<u64> {
        if sel == "@s" { return self.entities.first().map(|e| vec![e.id]).unwrap_or_default(); }
        if sel == "@p" {
            let mut ps: Vec<(u64, f64)> = self.entities.iter().filter(|e| e.is_player()).map(|e| { let dx=e.x-rx; let dy=e.y-ry; let dz=e.z-rz; (e.id, (dx*dx+dy*dy+dz*dz).sqrt()) }).collect();
            ps.sort_by(|a,b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            return ps.into_iter().map(|(id,_)| id).take(1).collect();
        }
        if sel == "@r" { return self.entities.iter().filter(|e| e.is_player()).map(|e| e.id).take(1).collect(); }
        if sel == "@a" { return self.entities.iter().filter(|e| e.is_player()).map(|e| e.id).collect(); }

        let (is_n, br) = if let Some(r) = sel.strip_prefix("@n") { (true, r) } else if let Some(r) = sel.strip_prefix("@e") { (false, r) } else { return vec![]; };
        let bracket = br.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or("");
        let f = parse_bracket_filters(bracket);
        let sort = if is_n && f.sort == SortOrder::Arbitrary { SortOrder::Nearest } else { f.sort };
        let limit = if is_n { f.limit.unwrap_or(1) } else { f.limit.unwrap_or(usize::MAX) };
        let rrx = f.ref_x.unwrap_or(rx); let rry = f.ref_y.unwrap_or(ry); let rrz = f.ref_z.unwrap_or(rz);

        let mut matches: Vec<(u64, f64)> = self.entities.iter()
            .filter(|e| entity_matches_filters(e, &f, rrx, rry, rrz, sb))
            .map(|e| { let dx=e.x-rrx; let dy=e.y-rry; let dz=e.z-rrz; (e.id, (dx*dx+dy*dy+dz*dz).sqrt()) }).collect();
        match sort { SortOrder::Nearest => matches.sort_by(|a,b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)), SortOrder::Furthest => matches.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)), SortOrder::Arbitrary => {} }
        matches.into_iter().map(|(id,_)| id).take(limit).collect()
    }
    pub fn count(&self, sel: &str, sb: &ScoreboardEngine) -> usize { self.resolve_selector(sel, sb).len() }

    // ── entity NBT ──

    pub fn nbt_get(&self, eid: u64, path: &str) -> Option<NbtValue> {
        let e = self.entities.iter().find(|e| e.id == eid)?;
        let ops = NbtValue::parse_path(path).ok()?;
        if let Some(NbtPathOp::Key(key)) = ops.first() {
            match key.as_str() {
                "Pos" => { let p = NbtValue::List(vec![NbtValue::Double(e.x),NbtValue::Double(e.y),NbtValue::Double(e.z)]); return p.resolve(&ops[1..]).cloned(); }
                "Rotation" => { let r = NbtValue::List(vec![NbtValue::Float(e.yaw),NbtValue::Float(e.pitch)]); return r.resolve(&ops[1..]).cloned(); }
                "Tags" => { let t = NbtValue::List(e.tags.iter().map(|t| NbtValue::String(t.clone())).collect()); return t.resolve(&ops[1..]).cloned(); }
                "id" => return Some(NbtValue::String(e.entity_type.clone())),
                "Dimension" => return Some(NbtValue::String(e.dimension.clone())),
                "UUID" => unimplemented!("UUID"),
                _ => {}
            }
        }
        e.nbt.resolve(&ops).cloned()
    }
    pub fn nbt_exists(&self, eid: u64, path: &str) -> bool { self.nbt_get(eid, path).is_some() }
    pub fn nbt_set(&mut self, eid: u64, path: &str, value: NbtValue) -> Result<(), String> {
        let e = self.entities.iter_mut().find(|e| e.id == eid).ok_or_else(|| format!("entity {eid} not found"))?;
        if path.is_empty() { e.nbt = value; return Ok(()); }
        let ops = NbtValue::parse_path(path)?;
        if ops.len() >= 1 { sync_entity_from_nbt_write(e, &ops, &value); }
        e.nbt.set_at(&ops, value)
    }
    pub fn nbt_remove(&mut self, eid: u64, path: &str) -> Result<(), String> {
        let e = self.entities.iter_mut().find(|e| e.id == eid).ok_or_else(|| format!("entity {eid} not found"))?;
        e.nbt.remove_at(&NbtValue::parse_path(path)?)
    }
    pub fn nbt_merge(&mut self, eid: u64, path: &str, source: NbtValue) -> Result<(), String> {
        let e = self.entities.iter_mut().find(|e| e.id == eid).ok_or_else(|| format!("entity {eid} not found"))?;
        if path.is_empty() {
            let NbtValue::Compound(inc) = source else { return Err("merge needs compound".into()); };
            let NbtValue::Compound(es) = &mut e.nbt else { return Err("not compound".into()); };
            for (k,v) in inc { if let Some(p) = es.iter().position(|(key,_)| key == &k) { es[p].1 = v; } else { es.push((k,v)); } }
            es.sort_by(|a,b| a.0.cmp(&b.0)); return Ok(());
        }
        e.nbt.merge_at(&NbtValue::parse_path(path)?, source)
    }
    pub fn nbt_append(&mut self, eid: u64, path: &str, value: NbtValue) -> Result<(), String> {
        let e = self.entities.iter_mut().find(|e| e.id == eid).ok_or_else(|| format!("entity {eid} not found"))?;
        e.nbt.append_at(&NbtValue::parse_path(path)?, value)
    }
    pub fn nbt_prepend(&mut self, eid: u64, path: &str, value: NbtValue) -> Result<(), String> {
        let e = self.entities.iter_mut().find(|e| e.id == eid).ok_or_else(|| format!("entity {eid} not found"))?;
        e.nbt.prepend_at(&NbtValue::parse_path(path)?, value)
    }
}

impl Default for EntityEngine { fn default() -> Self { Self::new() } }

// ── filter functions ──

fn parse_bracket_filters(bracket: &str) -> SelectorFilters {
    let mut f = SelectorFilters::default();
    for part in bracket.split(',') { let part = part.trim(); if part.is_empty() { continue; }
        let eq = find_eq_sep(part); if eq.is_none() { continue; } let eq = eq.unwrap();
        let key = part[..eq].trim(); let val = part[eq+1..].trim();
        match key {
            "type" => f.type_filter = Some(val.to_owned()), "tag" => f.tag_filter = Some(val.to_owned()),
            "limit" => f.limit = val.parse().ok(),
            "distance" => {
                if let Some(d) = val.strip_prefix("..") { if let Some(dot) = d.find("..") { f.distance_min = d[..dot].parse().ok(); f.distance_max = d[dot+2..].parse().ok(); } else { f.distance_max = d.parse().ok(); } }
                else if let Some(d) = val.strip_suffix("..") { f.distance_min = d.parse().ok(); }
                else if let Some(dot) = val.find("..") { f.distance_min = val[..dot].parse().ok(); f.distance_max = val[dot+2..].parse().ok(); }
                else { if let Ok(d) = val.parse() { f.distance_min = Some(d); f.distance_max = Some(d); } }
            }
            "x" => f.ref_x = val.parse().ok(), "y" => f.ref_y = val.parse().ok(), "z" => f.ref_z = val.parse().ok(),
            "dx" => f.dx = val.parse().ok(), "dy" => f.dy = val.parse().ok(), "dz" => f.dz = val.parse().ok(),
            "x_rotation" => { let (a,b) = parse_float_range(val); f.pitch_min = a; f.pitch_max = b; }
            "y_rotation" => { let (a,b) = parse_float_range(val); f.yaw_min = a; f.yaw_max = b; }
            "sort" => f.sort = match val { "nearest" => SortOrder::Nearest, "furthest" => SortOrder::Furthest, "arbitrary" => SortOrder::Arbitrary, _ => SortOrder::Arbitrary },
            "gamemode" => if let Some(v) = val.strip_prefix('!') { f.gamemode = Some(v.to_owned()); f.gamemode_negated = true; } else { f.gamemode = Some(val.to_owned()); },
            "nbt" => f.nbt_filter = NbtValue::from_snbt(val).ok(),
            "scores" => f.scores_filter = parse_scores_filter(val),
            _ => {}
        }
    }
    f
}

fn find_eq_sep(part: &str) -> Option<usize> {
    let mut d = 0u32; for (i,ch) in part.char_indices() { match ch { '{'|'[' => d += 1, '}'|']' => d = d.saturating_sub(1), '=' if d == 0 => return Some(i), _ => {} } } None
}
fn parse_float_range(val: &str) -> (Option<f32>, Option<f32>) {
    if let Some(r) = val.strip_prefix("..") { (None, r.parse().ok()) }
    else if let Some(r) = val.strip_suffix("..") { (r.parse().ok(), None) }
    else if let Some(dot) = val.find("..") { (val[..dot].parse().ok(), val[dot+2..].parse().ok()) }
    else { let v = val.parse().ok(); (v, v) }
}
fn parse_scores_filter(val: &str) -> Vec<(String, String)> {
    let inner = val.strip_prefix('{').and_then(|s| s.strip_suffix('}')).unwrap_or(val);
    if inner.is_empty() { return vec![]; }
    inner.split(',').map(|p| { let p = p.trim(); let eq = find_eq_sep(p).unwrap_or(p.len()); (p[..eq].trim().to_owned(), p[eq+1..].trim().to_owned()) }).collect()
}

fn entity_matches_filters(e: &Entity, f: &SelectorFilters, rx: f64, ry: f64, rz: f64, sb: &ScoreboardEngine) -> bool {
    if let Some(ref t) = f.type_filter { let ts = e.entity_type.strip_prefix("minecraft:").unwrap_or(&e.entity_type); if ts != t.as_str() && e.entity_type != *t && !(e.is_player() && t == "player") { return false; } }
    if let Some(ref t) = f.tag_filter { if !e.tags.contains(t) { return false; } }
    if let Some(ref m) = f.gamemode { if (e.gamemode_str() == m.as_str()) == f.gamemode_negated { return false; } }
    if f.distance_min.is_some() || f.distance_max.is_some() { let dx=e.x-rx; let dy=e.y-ry; let dz=e.z-rz; let dist=(dx*dx+dy*dy+dz*dz).sqrt(); if let Some(min)=f.distance_min { if dist<min { return false; } } if let Some(max)=f.distance_max { if dist>max { return false; } } }
    if f.dx.is_some()||f.dy.is_some()||f.dz.is_some() { if let Some(dx)=f.dx { if e.x<rx||e.x>rx+dx { return false; } } if let Some(dy)=f.dy { if e.y<ry||e.y>ry+dy { return false; } } if let Some(dz)=f.dz { if e.z<rz||e.z>rz+dz { return false; } } }
    if let Some(min)=f.yaw_min { if e.yaw<min { return false; } } if let Some(max)=f.yaw_max { if e.yaw>max { return false; } }
    if let Some(min)=f.pitch_min { if e.pitch<min { return false; } } if let Some(max)=f.pitch_max { if e.pitch>max { return false; } }
    if let Some(ref nbt)=f.nbt_filter { if !nbt_compound_subset(&e.nbt, nbt) { return false; } }
    for (obj, range) in &f.scores_filter { let v = sb.get(&format!("entity_{}",e.id),obj).or_else(|| sb.get(&e.id.to_string(),obj)).unwrap_or(0); if !eval_score_range(v,range) { return false; } }
    true
}

fn nbt_compound_subset(target: &NbtValue, pred: &NbtValue) -> bool {
    match (target, pred) {
        (NbtValue::Compound(t), NbtValue::Compound(p)) => p.iter().all(|(pk,pv)| t.iter().find(|(tk,_)| tk==pk).map(|(_,tv)| nbt_compound_subset(tv,pv)).unwrap_or(false)),
        (NbtValue::List(t), NbtValue::List(p)) => t.len()==p.len() && t.iter().zip(p.iter()).all(|(a,b)| nbt_compound_subset(a,b)),
        _ => target == pred,
    }
}
fn eval_score_range(v: i32, range: &str) -> bool {
    if range.is_empty() { return true; }
    if let Ok(n) = range.parse::<i32>() { return v == n; }
    if let Some(r) = range.strip_suffix("..") { return v >= r.parse().unwrap_or(i32::MIN); }
    if let Some(r) = range.strip_prefix("..") { return v <= r.parse().unwrap_or(i32::MAX); }
    if let Some(dot) = range.find("..") { let min = range[..dot].parse().unwrap_or(i32::MIN); let max = range[dot+2..].parse().unwrap_or(i32::MAX); return v >= min && v <= max; }
    false
}

fn sync_entity_from_nbt_write(e: &mut Entity, ops: &[NbtPathOp], val: &NbtValue) {
    if let NbtPathOp::Key(key) = &ops[0] { match key.as_str() { "Pos" => { if let NbtValue::List(vs) = val { if ops.len()==1 && vs.len()==3 { e.x=vs[0].as_f64(); e.y=vs[1].as_f64(); e.z=vs[2].as_f64(); } } } "Rotation" => { if let NbtValue::List(vs) = val { if ops.len()==1 && vs.len()==2 { e.yaw=vs[0].as_f32(); e.pitch=vs[1].as_f32(); } } } _ => {} } }
    if ops.len() >= 2 { if let (NbtPathOp::Key(key), NbtPathOp::Index(idx)) = (&ops[0], &ops[1]) { match (key.as_str(), *idx) { ("Pos",0)=>e.x=val.as_f64(), ("Pos",1)=>e.y=val.as_f64(), ("Pos",2)=>e.z=val.as_f64(), ("Rotation",0)=>e.yaw=val.as_f32(), ("Rotation",1)=>e.pitch=val.as_f32(), _=>{} } } }
}
