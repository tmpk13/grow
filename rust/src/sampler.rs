//! Sampling boxes.
//!
//! A sampler is a small drawable pixel grid that materials are sampled from.
//! Two layouts are supported and can be switched at any time:
//!
//!   Multi  - every sampler owns its own grid.
//!   Single - all samplers read from one shared atlas grid; each sampler owns a
//!            rectangular region of it.
//!
//! A sampler is read as a ramp: its unique colors sorted dark to light, indexed
//! by a tone value. How the colors are arranged in the grid does not matter,
//! only which distinct colors are present.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::util::{hsl_to_packed, luminance, packed_to_rgba_hex, rgba_hex_to_packed, EMPTY_COLOR};

pub struct RoleDef {
    pub id: &'static str,
    pub label: &'static str,
    pub hue: f64,
    pub sat: f64,
    pub l0: f64,
    pub l1: f64,
}

pub const ROLES: &[RoleDef] = &[
    RoleDef { id: "ground", label: "Ground cover", hue: 96.0, sat: 0.28, l0: 0.16, l1: 0.5 },
    RoleDef { id: "soil", label: "Soil", hue: 24.0, sat: 0.3, l0: 0.09, l1: 0.4 },
    RoleDef { id: "trunk", label: "Tree base / trunk", hue: 26.0, sat: 0.34, l0: 0.14, l1: 0.5 },
    RoleDef { id: "branch", label: "Branches", hue: 32.0, sat: 0.3, l0: 0.16, l1: 0.54 },
    RoleDef { id: "leaf", label: "Leaf texture", hue: 118.0, sat: 0.42, l0: 0.14, l1: 0.56 },
    RoleDef { id: "leafEdge", label: "Leaf edges", hue: 82.0, sat: 0.5, l0: 0.2, l1: 0.66 },
    RoleDef { id: "stem", label: "Stem to leaf", hue: 74.0, sat: 0.38, l0: 0.16, l1: 0.5 },
    RoleDef { id: "stone", label: "Stone", hue: 210.0, sat: 0.06, l0: 0.18, l1: 0.58 },
    RoleDef { id: "timber", label: "Timber wall", hue: 30.0, sat: 0.3, l0: 0.16, l1: 0.52 },
    RoleDef { id: "plank", label: "Sawn plank", hue: 38.0, sat: 0.34, l0: 0.22, l1: 0.66 },
    RoleDef { id: "thatch", label: "Thatch roof", hue: 46.0, sat: 0.42, l0: 0.2, l1: 0.62 },
    RoleDef { id: "brick", label: "Brick", hue: 14.0, sat: 0.44, l0: 0.18, l1: 0.56 },
    RoleDef { id: "metal", label: "Metal", hue: 205.0, sat: 0.1, l0: 0.24, l1: 0.74 },
    RoleDef { id: "cloth", label: "Cloth", hue: 330.0, sat: 0.26, l0: 0.24, l1: 0.68 },
];

pub fn role_def(id: &str) -> Option<&'static RoleDef> {
    ROLES.iter().find(|r| r.id == id)
}

pub fn role_label(id: &str) -> &str {
    role_def(id).map(|r| r.label).unwrap_or(id)
}

pub const DEFAULT_TONES: i32 = 6;

/// Pixel buffers travel through JSON as a run of RGBA hex quads, which is how
/// the project file has always stored them.
mod px_hex {
    use super::{packed_to_rgba_hex, rgba_hex_to_packed};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(px: &Vec<u32>, s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(px.len() * 8);
        for v in px {
            out.push_str(&packed_to_rgba_hex(*v));
        }
        s.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u32>, D::Error> {
        let raw = String::deserialize(d)?;
        let bytes = raw.as_bytes();
        let mut out = Vec::with_capacity(bytes.len() / 8);
        let mut at = 0;
        while at + 8 <= bytes.len() {
            out.push(rgba_hex_to_packed(&raw[at..at + 8]));
            at += 8;
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grid {
    pub w: i32,
    pub h: i32,
    #[serde(with = "px_hex")]
    pub px: Vec<u32>,
}

impl Grid {
    pub fn new(w: i32, h: i32) -> Self {
        Grid {
            w,
            h,
            px: vec![EMPTY_COLOR; (w * h).max(0) as usize],
        }
    }

    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            EMPTY_COLOR
        } else {
            self.px[(y * self.w + x) as usize]
        }
    }

    pub fn set(&mut self, x: i32, y: i32, v: u32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = v;
        }
    }

    /// Fits the buffer to the current size, keeping what already fits.
    pub fn fit(&mut self) {
        self.px.resize((self.w * self.h).max(0) as usize, EMPTY_COLOR);
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sampler {
    pub id: String,
    pub name: String,
    pub role: String,
    pub w: i32,
    pub h: i32,
    pub region: Region,
    #[serde(with = "px_hex")]
    pub px: Vec<u32>,
}

impl Sampler {
    pub fn new(id: &str, name: &str, role: &str, w: i32, h: i32, region: Region) -> Self {
        Sampler {
            id: id.to_string(),
            name: name.to_string(),
            role: role.to_string(),
            w,
            h,
            region,
            px: vec![EMPTY_COLOR; (w * h) as usize],
        }
    }

    /// Fills the box with a plausible starting ramp so the tool is usable
    /// before anything has been drawn. The lightness sweep is snapped to a
    /// small number of steps, so the box reads as pixel art and the resolved
    /// ramp stays short instead of holding one unique color per pixel.
    pub fn fill_default_art(&mut self, role: &RoleDef, seed_offset: i32, tones: i32) {
        let steps = tones.max(2).min(self.w * self.h).max(2);
        for y in 0..self.h {
            for x in 0..self.w {
                let u = if self.w > 1 { x as f64 / (self.w - 1) as f64 } else { 0.0 };
                let v = if self.h > 1 { y as f64 / (self.h - 1) as f64 } else { 0.0 };
                let dither = (((x * 7 + y * 13 + seed_offset).rem_euclid(3)) - 1) as f64 * 0.06;
                let t = (u + dither + (v - 0.5) * 0.08).clamp(0.0, 1.0);
                let idx = (t * (steps - 1) as f64).round();
                let f = idx / (steps - 1) as f64;
                let l = role.l0 + (role.l1 - role.l0) * f;
                let hue = role.hue + (f - 0.5) * 10.0;
                let sat = role.sat * (1.0 - f * 0.12);
                self.px[(y * self.w + x) as usize] = hsl_to_packed(hue, sat, l);
            }
        }
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        let mut next = vec![EMPTY_COLOR; (w * h) as usize];
        for y in 0..h {
            let sy = if self.h > 1 {
                ((y * self.h) / h).min(self.h - 1)
            } else {
                0
            };
            for x in 0..w {
                let sx = if self.w > 1 {
                    ((x * self.w) / w).min(self.w - 1)
                } else {
                    0
                };
                next[(y * w + x) as usize] = self.px[(sy * self.w + sx) as usize];
            }
        }
        self.px = next;
        self.w = w;
        self.h = h;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaterialMode {
    Multi,
    Single,
}

#[derive(Default)]
struct RampCache {
    key: (u32, bool),
    map: HashMap<String, Rc<Vec<u32>>>,
}

#[derive(Serialize, Deserialize)]
pub struct Materials {
    pub mode: MaterialMode,
    pub atlas: Grid,
    pub samplers: Vec<Sampler>,
    #[serde(skip, default = "one")]
    pub version: u32,
    #[serde(skip)]
    cache: RefCell<RampCache>,
}

fn one() -> u32 {
    1
}

impl Clone for Materials {
    fn clone(&self) -> Self {
        Materials {
            mode: self.mode,
            atlas: self.atlas.clone(),
            samplers: self.samplers.clone(),
            version: self.version,
            cache: RefCell::new(RampCache::default()),
        }
    }
}

const BAND_H: i32 = 3;

impl Default for Materials {
    fn default() -> Self {
        Self::new()
    }
}

impl Materials {
    /// The shared grid is sized so every role gets a band of equal height with
    /// no leftover rows.
    pub fn new() -> Self {
        let atlas_w = 24;
        let atlas_h = BAND_H * ROLES.len() as i32;
        let mut materials = Materials {
            mode: MaterialMode::Multi,
            atlas: Grid::new(atlas_w, atlas_h),
            samplers: Vec::new(),
            version: 1,
            cache: RefCell::new(RampCache::default()),
        };
        for (i, role) in ROLES.iter().enumerate() {
            let mut s = Sampler::new(
                &format!("mat-{}", role.id),
                role.label,
                role.id,
                16,
                6,
                Region { x: 0, y: i as i32 * BAND_H, w: atlas_w, h: BAND_H },
            );
            s.fill_default_art(role, i as i32 * 31, DEFAULT_TONES);
            materials.samplers.push(s);
        }
        materials.paint_atlas_from_samplers();
        materials
    }

    pub fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
        self.invalidate();
    }

    pub fn invalidate(&self) {
        let mut cache = self.cache.borrow_mut();
        cache.map.clear();
        cache.key = (u32::MAX, false);
    }

    pub fn find(&self, id: &str) -> Option<&Sampler> {
        self.samplers.iter().find(|s| s.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Sampler> {
        self.samplers.iter_mut().find(|s| s.id == id)
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.samplers.iter().position(|s| s.id == id)
    }

    /// A project saved before a role existed has no sampler for it. Rather than
    /// leaving that material unpainted, the missing boxes are appended with
    /// their default art and the shared atlas is grown to fit their bands.
    pub fn ensure_role_samplers(&mut self) {
        let missing: Vec<usize> = ROLES
            .iter()
            .enumerate()
            .filter(|(_, r)| !self.samplers.iter().any(|s| s.role == r.id))
            .map(|(i, _)| i)
            .collect();
        if missing.is_empty() {
            return;
        }
        let needed_h = ROLES.len() as i32 * BAND_H;
        if self.atlas.h < needed_h {
            self.atlas.h = needed_h;
            self.atlas.fit();
        }
        for index in missing {
            let role = &ROLES[index];
            let mut s = Sampler::new(
                &format!("mat-{}", role.id),
                role.label,
                role.id,
                16,
                6,
                Region { x: 0, y: index as i32 * BAND_H, w: self.atlas.w, h: BAND_H },
            );
            s.fill_default_art(role, index as i32 * 31, DEFAULT_TONES);
            self.samplers.push(s);
        }
        self.touch();
    }

    /// Copies each sampler's own art into its atlas region, so switching to the
    /// shared grid starts from what the separate boxes already show.
    pub fn paint_atlas_from_samplers(&mut self) {
        self.atlas.px.fill(EMPTY_COLOR);
        let atlas_w = self.atlas.w;
        let atlas_h = self.atlas.h;
        for s in &self.samplers {
            let r = s.region;
            for y in 0..r.h {
                let ay = r.y + y;
                if ay < 0 || ay >= atlas_h {
                    continue;
                }
                let sy = if s.h > 1 {
                    ((y * s.h) / r.h.max(1)).min(s.h - 1)
                } else {
                    0
                };
                for x in 0..r.w {
                    let ax = r.x + x;
                    if ax < 0 || ax >= atlas_w {
                        continue;
                    }
                    let sx = if s.w > 1 {
                        ((x * s.w) / r.w.max(1)).min(s.w - 1)
                    } else {
                        0
                    };
                    self.atlas.px[(ay * atlas_w + ax) as usize] = s.px[(sy * s.w + sx) as usize];
                }
            }
        }
        self.touch();
    }

    pub fn copy_atlas_to_samplers(&mut self) {
        let atlas = self.atlas.clone();
        for s in &mut self.samplers {
            let r = s.region;
            s.resize(r.w.max(1), r.h.max(1));
            for y in 0..r.h {
                for x in 0..r.w {
                    let v = atlas.get(r.x + x, r.y + y);
                    s.px[(y * s.w + x) as usize] = v;
                }
            }
        }
        self.touch();
    }

    /// The pixel buffer a sampler currently reads from, honoring the mode.
    pub fn patch(&self, sampler: &Sampler) -> Grid {
        if self.mode == MaterialMode::Multi {
            return Grid { w: sampler.w, h: sampler.h, px: sampler.px.clone() };
        }
        let r = sampler.region;
        let w = r.w.min(self.atlas.w - r.x).max(1);
        let h = r.h.min(self.atlas.h - r.y).max(1);
        let mut px = vec![EMPTY_COLOR; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                px[(y * w + x) as usize] = self.atlas.get(r.x + x, r.y + y);
            }
        }
        Grid { w, h, px }
    }

    /// Unique opaque colors of a sampler, sorted dark to light. Cached per
    /// materials version so the sim can call this for every pixel.
    pub fn ramp(&self, id: &str) -> Rc<Vec<u32>> {
        let key = (self.version, self.mode == MaterialMode::Single);
        {
            let mut cache = self.cache.borrow_mut();
            if cache.key != key {
                cache.key = key;
                cache.map.clear();
            }
            if let Some(hit) = cache.map.get(id) {
                return hit.clone();
            }
        }
        let colors = match self.find(id) {
            Some(sampler) => {
                let patch = self.patch(sampler);
                let mut seen: Vec<u32> = Vec::new();
                for v in patch.px {
                    if v == EMPTY_COLOR || seen.contains(&v) {
                        continue;
                    }
                    seen.push(v);
                }
                seen.sort_by(|a, b| luminance(*a).partial_cmp(&luminance(*b)).unwrap());
                seen
            }
            None => Vec::new(),
        };
        let rc = Rc::new(colors);
        self.cache.borrow_mut().map.insert(id.to_string(), rc.clone());
        rc
    }
}

pub fn ramp_pick(ramp: &[u32], t: f64) -> u32 {
    if ramp.is_empty() {
        return EMPTY_COLOR;
    }
    let i = (t * (ramp.len() - 1) as f64).round();
    let i = if i < 0.0 {
        0
    } else if i as usize >= ramp.len() {
        ramp.len() - 1
    } else {
        i as usize
    };
    ramp[i]
}
