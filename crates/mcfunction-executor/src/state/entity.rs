/// Minimal entity registry — supports summon, tagging, selector matching,
/// and position updates via teleport/execute modifiers.
#[derive(Debug, Clone)]
pub struct EntityEngine {
    entities: Vec<Entity>,
    next_id: u64,
}

#[derive(Debug, Clone)]
pub struct Entity {
    pub id: u64,
    pub entity_type: String,
    pub tags: Vec<String>,
    pub dimension: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl EntityEngine {
    pub fn new() -> Self {
        Self { entities: Vec::new(), next_id: 1 }
    }

    pub fn summon(
        &mut self,
        entity_type: &str,
        x: f64,
        y: f64,
        z: f64,
        tags: &[String],
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.entities.push(Entity {
            id,
            entity_type: entity_type.to_owned(),
            tags: tags.to_vec(),
            dimension: "minecraft:overworld".to_owned(),
            x, y, z,
            yaw: 0.0,
            pitch: 0.0,
        });
        id
    }

    pub fn add_tag(&mut self, entity_id: u64, tag: &str) {
        if let Some(entity) = self.entities.iter_mut().find(|e| e.id == entity_id) {
            if !entity.tags.contains(&tag.to_owned()) {
                entity.tags.push(tag.to_owned());
            }
        }
    }

    // ── position mutation ─────────────────────────────────────

    pub fn teleport_absolute(&mut self, entity_id: u64, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == entity_id) {
            e.x = x;
            e.y = y;
            e.z = z;
            e.yaw = yaw;
            e.pitch = pitch;
        }
    }

    pub fn teleport_relative(&mut self, entity_id: u64, dx: f64, dy: f64, dz: f64) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == entity_id) {
            e.x += dx;
            e.y += dy;
            e.z += dz;
        }
    }

    pub fn teleport_local(&mut self, entity_id: u64, left: f64, up: f64, forward: f64, anchor_eyes: bool) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == entity_id) {
            let yaw = e.yaw as f64;
            // Local ^ coordinates: left=-X, forward=+Z in rotated frame
            let yaw_rad = yaw.to_radians();
            let cos = yaw_rad.cos();
            let sin = yaw_rad.sin();
            let eye_offset = if anchor_eyes { 1.27 } else { 0.0 };
            e.x += forward * sin - left * cos;
            e.y += up + eye_offset;
            e.z += forward * cos + left * sin;
        }
    }

    pub fn set_rotation(&mut self, entity_id: u64, yaw: f32, pitch: f32) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == entity_id) {
            e.yaw = yaw;
            e.pitch = pitch;
        }
    }

    pub fn set_dimension(&mut self, entity_id: u64, dimension: &str) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == entity_id) {
            e.dimension = dimension.to_owned();
        }
    }

    #[must_use]
    pub fn get(&self, id: u64) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    // ── selector resolution ───────────────────────────────────

    pub fn resolve_selector(&self, selector: &str) -> Vec<u64> {
        if selector == "@s" {
            return self.entities.first().map(|e| vec![e.id]).unwrap_or_default();
        }
        if selector == "@p" || selector == "@r" {
            return self.entities.first().map(|e| vec![e.id]).unwrap_or_default();
        }
        if selector == "@a" {
            return self.entities.iter().map(|e| e.id).collect();
        }
        if !selector.starts_with("@e") {
            return vec![];
        }
        let bracket = selector.strip_prefix("@e")
            .and_then(|s| s.strip_prefix('['))
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or("");

        let mut limit = usize::MAX;
        let mut filter_type: Option<String> = None;
        let mut filter_tag: Option<String> = None;
        let mut filter_distance: Option<f64> = None; // distance=..N or distance=N
        let mut filter_x: Option<f64> = None;
        let mut filter_y: Option<f64> = None;
        let mut filter_z: Option<f64> = None;

        for part in bracket.split(',') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix("type=") {
                filter_type = Some(v.to_owned());
            } else if let Some(v) = part.strip_prefix("tag=") {
                filter_tag = Some(v.to_owned());
            } else if let Some(v) = part.strip_prefix("limit=") {
                limit = v.parse().unwrap_or(usize::MAX);
            } else if let Some(v) = part.strip_prefix("distance=") {
                if let Some(d) = v.strip_prefix("..") {
                    // ..N — max distance
                    filter_distance = d.parse().ok();
                } else if let Some(d) = v.strip_suffix("..") {
                    // N.. — min distance (rare, skip for now)
                    filter_distance = None;
                } else {
                    filter_distance = v.parse().ok();
                }
            } else if let Some(v) = part.strip_prefix("x=") {
                filter_x = v.parse().ok();
            } else if let Some(v) = part.strip_prefix("y=") {
                filter_y = v.parse().ok();
            } else if let Some(v) = part.strip_prefix("z=") {
                filter_z = v.parse().ok();
            }
        }

        let mut matches: Vec<u64> = self.entities.iter()
            .filter(|e| {
                if let Some(ref t) = filter_type {
                    let type_short = e.entity_type.strip_prefix("minecraft:").unwrap_or(&e.entity_type);
                    if type_short != t.as_str() && e.entity_type != *t {
                        return false;
                    }
                }
                if let Some(ref t) = filter_tag {
                    if !e.tags.contains(t) {
                        return false;
                    }
                }
                if let Some(d) = filter_distance {
                    // Use the generic reference position
                    let ref_x = filter_x.unwrap_or(0.0);
                    let ref_y = filter_y.unwrap_or(0.0);
                    let ref_z = filter_z.unwrap_or(0.0);
                    let dx = e.x - ref_x;
                    let dy = e.y - ref_y;
                    let dz = e.z - ref_z;
                    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                    if dist > d {
                        return false;
                    }
                }
                true
            })
            .map(|e| e.id)
            .take(limit)
            .collect();
        matches.sort();
        matches
    }

    pub fn count(&self, selector: &str) -> usize {
        self.resolve_selector(selector).len()
    }

    /// Compute x/y/z/distance filters relative to a given reference position
    pub fn resolve_selector_at(
        &self,
        selector: &str,
        ref_x: f64,
        ref_y: f64,
        ref_z: f64,
    ) -> Vec<u64> {
        // For now delegate to resolve_selector — the @e[... x=...] syntax handles it
        let _ = (ref_x, ref_y, ref_z);
        self.resolve_selector(selector)
    }
}

impl Default for EntityEngine {
    fn default() -> Self {
        Self::new()
    }
}
