//! A growing plant instance.
//!
//! Growth happens in the plant's own pixel space (a sprite buffer anchored at
//! the root pixel, bottom center). Each growth step advances one active tip,
//! which may branch, droop, climb a support or terminate into a leaf cluster.
//!
//! Rendering is a two stage process:
//!   1. rasterize segments and leaf blobs into a material id mask
//!   2. shade every pixel from its depth inside its own shape and its vertical
//!      position inside that shape, then look the tone up in the sampling box
//!      assigned to that material
//!
//! Step 2 treats trunk, branch and stem as one body and leaf plus leaf edge as
//! another, so a leaf is shaded as a leaf and not as part of the branch it
//! hangs off.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::rng::Rng;
use crate::sampler::Bands;
use crate::shading::{curve_value, quantize, Shading};
use crate::species::{EffectiveLimits, SizeClass, Species};
use crate::util::{
    clamp, clamp01, distance_transform, hash2, label_components, pack_rgba, to_rad, unpack_rgba,
    EMPTY_COLOR,
};
use crate::world::{Support, World};

/// How many times a shrivel is re-drawn from start to finish. Enough that it
/// reads as drying out, few enough that a field of plants dying at once does
/// not cost a raster each per frame.
const SHRIVEL_STEPS: f64 = 12.0;

/// What everything fades to on the way out: dry straw, not the green it was.
const DEAD_COLOR: u32 = pack_rgba(120, 98, 66, 255);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum Mat {
    Empty = 0,
    Trunk = 1,
    Branch = 2,
    Leaf = 3,
    LeafEdge = 4,
    Stem = 5,
    Ground = 6,
}

pub const MAT_COUNT: usize = 7;

impl Mat {
    pub fn from_u8(v: u8) -> Mat {
        match v {
            1 => Mat::Trunk,
            2 => Mat::Branch,
            3 => Mat::Leaf,
            4 => Mat::LeafEdge,
            5 => Mat::Stem,
            6 => Mat::Ground,
            _ => Mat::Empty,
        }
    }

    pub fn all() -> [Mat; 6] {
        [Mat::Trunk, Mat::Branch, Mat::Leaf, Mat::LeafEdge, Mat::Stem, Mat::Ground]
    }
}

/// Materials shaded together as one body.
struct ShadeGroup {
    mats: &'static [Mat],
    wood: bool,
}

const SHADE_GROUPS: &[ShadeGroup] = &[
    ShadeGroup { mats: &[Mat::Trunk, Mat::Branch, Mat::Stem], wood: true },
    ShadeGroup { mats: &[Mat::Leaf, Mat::LeafEdge], wood: false },
    ShadeGroup { mats: &[Mat::Ground], wood: true },
];

/// The shade group each material belongs to, indexed by `Mat`; the entry for
/// `Empty` is never read.
const GROUP_OF: [usize; MAT_COUNT] = [0, 0, 0, 1, 1, 0, 2];

const SPRITE_PAD: f64 = 4.0;

pub const SUPPORT_LAYERS: [usize; 2] = [2, 3];

fn angle_diff(target: f64, current: f64) -> f64 {
    let mut d = target - current;
    while d > std::f64::consts::PI {
        d -= 2.0 * std::f64::consts::PI;
    }
    while d < -std::f64::consts::PI {
        d += 2.0 * std::f64::consts::PI;
    }
    d
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(from = "SegmentWire", into = "SegmentWire")]
pub struct Segment {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub w: f64,
    pub mat: Mat,
    pub bias: i8,
}

/// Woody segments are the bulk of a saved world by a wide margin, so the three
/// growing shapes travel as bare arrays of numbers rather than as objects with
/// a name against every field. The saving is most of the file.
#[derive(Serialize, Deserialize)]
struct SegmentWire(f64, f64, f64, f64, f64, u8, i8);

impl From<Segment> for SegmentWire {
    fn from(s: Segment) -> SegmentWire {
        SegmentWire(s.x0, s.y0, s.x1, s.y1, s.w, s.mat as u8, s.bias)
    }
}

impl From<SegmentWire> for Segment {
    fn from(w: SegmentWire) -> Segment {
        Segment { x0: w.0, y0: w.1, x1: w.2, y1: w.3, w: w.4, mat: Mat::from_u8(w.5), bias: w.6 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(from = "LeafWire", into = "LeafWire")]
pub struct Leaf {
    pub x: f64,
    pub y: f64,
    pub rx: f64,
    pub ry: f64,
    pub seed: u32,
    pub bias: i8,
}

#[derive(Serialize, Deserialize)]
struct LeafWire(f64, f64, f64, f64, u32, i8);

impl From<Leaf> for LeafWire {
    fn from(l: Leaf) -> LeafWire {
        LeafWire(l.x, l.y, l.rx, l.ry, l.seed, l.bias)
    }
}

impl From<LeafWire> for Leaf {
    fn from(w: LeafWire) -> Leaf {
        Leaf { x: w.0, y: w.1, rx: w.2, ry: w.3, seed: w.4, bias: w.5 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(from = "TipWire", into = "TipWire")]
pub struct Tip {
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    pub width: f64,
    pub depth: i32,
    pub len: f64,
    pub since_branch: f64,
    pub phase: f64,
    pub dir: f64,
    pub support: Option<Support>,
    pub alive: bool,
}

#[derive(Serialize, Deserialize)]
struct TipWire(f64, f64, f64, f64, i32, f64, f64, f64, f64, Option<Support>, bool);

impl From<Tip> for TipWire {
    fn from(t: Tip) -> TipWire {
        TipWire(
            t.x,
            t.y,
            t.angle,
            t.width,
            t.depth,
            t.len,
            t.since_branch,
            t.phase,
            t.dir,
            t.support,
            t.alive,
        )
    }
}

impl From<TipWire> for Tip {
    fn from(w: TipWire) -> Tip {
        Tip {
            x: w.0,
            y: w.1,
            angle: w.2,
            width: w.3,
            depth: w.4,
            len: w.5,
            since_branch: w.6,
            phase: w.7,
            dir: w.8,
            support: w.9,
            alive: w.10,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bounds {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Bounds {
    pub fn is_empty(&self) -> bool {
        self.x1 < self.x0
    }

    /// Widens the box to take the pixel in.
    pub fn include(&mut self, x: i32, y: i32) {
        if self.is_empty() {
            *self = Bounds { x0: x, y0: y, x1: x, y1: y };
            return;
        }
        if x < self.x0 {
            self.x0 = x;
        }
        if x > self.x1 {
            self.x1 = x;
        }
        if y < self.y0 {
            self.y0 = y;
        }
        if y > self.y1 {
            self.y1 = y;
        }
    }
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds { x0: 0, y0: 0, x1: -1, y1: -1 }
    }
}

/// Reusable working buffers for the shading pass, shared by every plant in a
/// sim so the per plant cost stays in the sprite itself.
#[derive(Default)]
pub struct Scratch {
    gmask: Vec<u8>,
    dist: Vec<f32>,
    labels: Vec<i32>,
    stack: Vec<usize>,
    /// Per component: the vertical fraction and the two vertical curve terms,
    /// valid for the row named in `vstamp`.
    vcache: Vec<(f64, f64, f64)>,
    vstamp: Vec<u32>,
}

/// The sampling boxes a species resolves to, indexed by material, each read as
/// a set of vertical bands.
pub type Ramps = [Rc<Bands>; MAT_COUNT];

pub struct RasterEnv<'a> {
    pub shading: &'a Shading,
    pub ramps: &'a Ramps,
}

/// A saved plant carries its shape and the stream that grew it; the pixels it
/// was last drawn into are left out and painted again on the way back in.
#[derive(Serialize, Deserialize)]
pub struct Plant {
    pub id: i32,
    pub species_id: String,
    pub size_class: SizeClass,
    pub limits: EffectiveLimits,
    pub col: i32,
    pub row: i32,
    pub layer: usize,
    pub rng: Rng,
    pub seed: u32,
    pub age: f64,
    pub alive: bool,
    /// How far through drying out this plant is, 0 while it is still growing
    /// and 1 when there is nothing of it left. Nothing on the map disappears
    /// between one frame and the next: past its age a plant browns and comes
    /// apart from the tips down, and is only taken away when this reaches 1.
    pub wither: f64,
    pub budget: f64,
    /// Atmospheric lift for far rows, set by the sim.
    pub depth_shade: f64,
    pub growth_rate: f64,
    pub cell_px: i32,
    pub depth_ratio: f64,
    pub max_radius_px: f64,
    pub max_radius_y_px: f64,
    pub w: i32,
    pub h: i32,
    pub ox: i32,
    pub oy: i32,
    pub segments: Vec<Segment>,
    pub leaves: Vec<Leaf>,
    pub tips: Vec<Tip>,
    pub granted_radius_cells: i32,
    pub cells: Vec<usize>,
    pub confined_side: bool,
    pub radius_px: f64,
    pub height_px: f64,
    #[serde(skip)]
    pub mask: Vec<u8>,
    #[serde(skip)]
    pub bias: Vec<i8>,
    #[serde(skip)]
    pub sprite: Vec<u32>,
    /// The rectangle everything ever stamped fell inside. A plant fills a
    /// small corner of its own fixed-size buffer for most of its life, and
    /// outside this rectangle the buffer is known to be empty, so every pass
    /// of the raster confines itself to it rather than sweeping the whole
    /// buffer.
    #[serde(skip)]
    pub stamped: Bounds,
    /// Segments and leaves are only ever appended while a plant grows, so
    /// they are stamped once each into a wood plane and a leaf plane, and a
    /// raster composites the two rather than re-stamping the whole history.
    /// The planes are laid out like `mask` and carry the material per pixel;
    /// the counters say how much of each list has been stamped so far.
    #[serde(skip)]
    wood_mask: Vec<u8>,
    #[serde(skip)]
    wood_bias: Vec<i8>,
    #[serde(skip)]
    leaf_mask: Vec<u8>,
    #[serde(skip)]
    leaf_bias: Vec<i8>,
    #[serde(skip)]
    stamped_segments: usize,
    #[serde(skip)]
    stamped_leaves: usize,
    /// Where this plant's species sat in the species list when it was last
    /// looked up. The list almost never changes, so checking the remembered
    /// slot first turns a scan per plant per tick into a scan per edit.
    #[serde(skip)]
    pub species_hint: usize,
    /// How many tips are still growing. Kept as a count because the whole
    /// list, dead tips included, is what maturity would otherwise scan every
    /// tick of every plant's life.
    #[serde(skip)]
    alive_tips: i32,
    pub bounds: Bounds,
    pub dirty: bool,
    /// Settler currently on their way to cut this plant down.
    pub claimed_by: u32,
    /// How far through coming down this plant is, from 0 while it stands to 1
    /// when it is off the map. A cut plant is not taken away where it stood:
    /// it tips over from the foot first, and only then goes. Nothing else in
    /// the world sees a plant that is falling - it is out of the index, out of
    /// the way and past being cut again - so this is the one flag that says
    /// the sprite is drawn turned rather than upright.
    #[serde(default)]
    pub felled: f64,
    /// One color standing for the whole plant, averaged over the sprite at
    /// raster time. Zoomed far enough out a plant is drawn as this and nothing
    /// else, which is what keeps a forest of thousands legible and cheap.
    pub tint: u32,
}

impl Plant {
    pub fn new(
        id: i32,
        species: &Species,
        limits: EffectiveLimits,
        col: i32,
        row: i32,
        world: &World,
        mut rng: Rng,
    ) -> Self {
        let (cell_px, depth_ratio) = (world.cell_px, world.depth_ratio);
        let seed = rng.seed();
        let growth_rate = rng.range(species.growth.rate_min, species.growth.rate_max);
        let cell = cell_px as f64;
        let max_radius_px = limits.max_radius_cells as f64 * cell + cell / 2.0;
        let w = (max_radius_px * 2.0 + SPRITE_PAD * 2.0).ceil() as i32;
        let ox = w / 2;
        let (max_radius_y_px, h, oy) = if species.size_class == SizeClass::Ground {
            // A mat lies flat on the ground plane, so its sprite is a
            // foreshortened disc centered on the anchor instead of a shape
            // standing on it.
            let ry = (max_radius_px * depth_ratio).max(1.0);
            let h = (ry * 2.0 + limits.max_height_px + SPRITE_PAD * 2.0).ceil() as i32;
            (ry, h, (SPRITE_PAD + ry).round() as i32)
        } else {
            let h = (limits.max_height_px + SPRITE_PAD * 2.0).ceil() as i32;
            (0.0, h, h - SPRITE_PAD as i32)
        };

        let n = (w * h) as usize;
        let mut plant = Plant {
            id,
            species_id: species.id.clone(),
            size_class: species.size_class,
            limits,
            col,
            row,
            layer: species.size_class.layer(),
            rng,
            seed,
            age: 0.0,
            alive: true,
            budget: 0.0,
            depth_shade: 0.0,
            growth_rate,
            cell_px,
            depth_ratio,
            max_radius_px,
            max_radius_y_px,
            w,
            h,
            ox,
            oy,
            segments: Vec::new(),
            leaves: Vec::new(),
            tips: Vec::new(),
            granted_radius_cells: 0,
            cells: Vec::new(),
            confined_side: false,
            radius_px: 0.0,
            height_px: 0.0,
            mask: vec![0; n],
            bias: vec![0; n],
            sprite: vec![EMPTY_COLOR; n],
            stamped: Bounds::default(),
            wood_mask: Vec::new(),
            wood_bias: Vec::new(),
            leaf_mask: Vec::new(),
            leaf_bias: Vec::new(),
            stamped_segments: 0,
            stamped_leaves: 0,
            species_hint: 0,
            alive_tips: 0,
            bounds: Bounds { x0: 0, y0: 0, x1: -1, y1: -1 },
            dirty: true,
            wither: 0.0,
            claimed_by: 0,
            felled: 0.0,
            tint: 0,
        };
        plant.init_tips(species);
        plant
    }

    fn init_tips(&mut self, species: &Species) {
        if species.size_class == SizeClass::Ground {
            return;
        }
        let jitter = to_rad(self.rng.range(-6.0, 6.0));
        let phase = self.rng.range(0.0, std::f64::consts::PI * 2.0);
        let dir = self.rng.sign();
        self.tips.push(Tip {
            x: self.ox as f64,
            y: self.oy as f64,
            angle: -std::f64::consts::FRAC_PI_2 + jitter,
            width: species.form.base_width,
            depth: 0,
            len: 0.0,
            since_branch: 0.0,
            phase,
            dir,
            support: None,
            alive: true,
        });
        self.alive_tips += 1;
    }

    /// Upright and part of the world. A plant that has been cut is neither.
    pub fn standing(&self) -> bool {
        self.alive && self.felled <= 0.0
    }

    /// Which way it goes over, from its own seed, so the same tree in the same
    /// world always falls the same way.
    pub fn fall_dir(&self) -> f64 {
        if self.seed.is_multiple_of(2) {
            1.0
        } else {
            -1.0
        }
    }

    pub fn alive_tip_count(&self) -> i32 {
        self.alive_tips
    }

    /// The index of this plant's species in the list, trying the remembered
    /// slot before scanning. None when the species has been deleted.
    pub fn species_index(&mut self, species: &[Species]) -> Option<usize> {
        match species.get(self.species_hint) {
            Some(s) if s.id == self.species_id => Some(self.species_hint),
            _ => {
                let i = species.iter().position(|s| s.id == self.species_id)?;
                self.species_hint = i;
                Some(i)
            }
        }
    }

    pub fn mature(&self) -> bool {
        if self.size_class != SizeClass::Ground {
            return self.alive_tip_count() == 0;
        }
        let spread = self.radius_px >= self.max_radius_px || self.confined_side;
        spread && self.height_px >= self.limits.max_height_px
    }

    pub fn grow(&mut self, dt: f64, species: &Species, world: &mut World, blocked: Option<&[u8]>) {
        if !self.alive {
            return;
        }
        self.age += dt;
        if self.age > species.growth.max_age {
            self.shrivel(dt, species.growth.shrivel);
            return;
        }
        if self.mature() {
            return;
        }
        self.budget += self.growth_rate * dt;
        let mut guard = 0;
        while self.budget >= 1.0 && guard < 64 {
            self.budget -= 1.0;
            guard += 1;
            if self.size_class == SizeClass::Ground {
                self.step_ground(world, blocked);
            } else {
                self.step_branching(species, world, blocked);
            }
            if self.mature() {
                break;
            }
        }
    }

    /// A patch thickens whether or not it can still spread sideways, so a mat
    /// hemmed in by neighbors still fills out instead of staying one pixel tall.
    fn step_ground(&mut self, world: &mut World, blocked: Option<&[u8]>) {
        if self.height_px < self.limits.max_height_px {
            self.height_px = (self.height_px + 0.75).min(self.limits.max_height_px);
            self.dirty = true;
        }
        if self.radius_px >= self.max_radius_px {
            return;
        }
        let next = self.radius_px + 1.0;
        let cells = (next / self.cell_px as f64).ceil() as i32;
        if cells > self.granted_radius_cells && !self.request_space(cells, world, blocked) {
            self.confined_side = true;
            return;
        }
        self.radius_px = next;
        self.dirty = true;
    }

    fn step_branching(&mut self, species: &Species, world: &mut World, blocked: Option<&[u8]>) {
        let alive: Vec<usize> = (0..self.tips.len()).filter(|&i| self.tips[i].alive).collect();
        if alive.is_empty() {
            return;
        }
        let pick = (self.rng.next() * alive.len() as f64).floor() as usize % alive.len();
        self.advance_tip(alive[pick], species, world, blocked);
        self.dirty = true;
    }

    fn advance_tip(&mut self, ti: usize, species: &Species, world: &mut World, blocked: Option<&[u8]>) {
        let f = species.form;
        let mut tip = self.tips[ti];

        tip.angle += to_rad(self.rng.range(-f.wander, f.wander)) * 0.5;
        tip.angle += angle_diff(-std::f64::consts::FRAC_PI_2, tip.angle) * f.phototropism * 0.3;
        if tip.depth > 0 {
            tip.angle +=
                angle_diff(std::f64::consts::FRAC_PI_2, tip.angle) * f.gravity * 0.12 * tip.depth as f64;
        }

        let mut behind = false;
        if f.wrap {
            behind = self.steer_climb(&mut tip, species, world);
        }

        let step = self.rng.range(species.growth.step_min, species.growth.step_max);
        let mut nx = tip.x + tip.angle.cos() * step;
        let mut ny = tip.y + tip.angle.sin() * step;

        // Spreading wider needs ground cells; when the world will not grant
        // them the tip is steered back inward instead of stopping dead, so a
        // crowded plant grows tall and narrow.
        let want_radius = (nx - self.ox as f64).abs();
        if want_radius > self.granted_radius_cells as f64 * self.cell_px as f64 + self.cell_px as f64 / 2.0 {
            let cells = self
                .limits
                .max_radius_cells
                .min((want_radius / self.cell_px as f64).ceil() as i32);
            if cells > self.granted_radius_cells && !self.request_space(cells, world, blocked) {
                self.confined_side = true;
                tip.angle += angle_diff(-std::f64::consts::FRAC_PI_2, tip.angle) * 0.6;
                nx = tip.x + tip.angle.cos() * step;
                ny = tip.y + tip.angle.sin() * step;
            }
        }

        let limit_x = self.max_radius_px;
        if (nx - self.ox as f64).abs() > limit_x
            || ny < self.oy as f64 - self.limits.max_height_px
            || ny > self.oy as f64 + 2.0
        {
            self.end_tip(&mut tip, species);
            self.tips[ti] = tip;
            return;
        }

        let width = tip.width.max(f.min_width);
        let mat = if tip.depth == 0 { Mat::Trunk } else { Mat::Branch };
        let bias = if behind {
            -((species.shade.behind_shade * 100.0).round() as i8)
        } else {
            0
        };
        self.segments.push(Segment { x0: tip.x, y0: tip.y, x1: nx, y1: ny, w: width, mat, bias });

        tip.x = nx;
        tip.y = ny;
        tip.len += step;
        tip.since_branch += step;
        tip.width *= f.taper;
        self.radius_px = self.radius_px.max((nx - self.ox as f64).abs());
        self.height_px = self.height_px.max(self.oy as f64 - ny);

        if tip.depth >= f.leaf_depth && self.rng.chance(f.leaf_density) {
            self.add_leaf(&tip, behind, species);
        }

        if tip.since_branch >= f.branch_interval
            && tip.depth < f.max_depth
            && self.alive_tip_count() < self.limits.max_tips
            && self.rng.chance(f.branch_chance)
        {
            self.branch(&mut tip, species);
        }

        if tip.width < f.min_width || tip.len > self.limits.max_height_px * 1.6 {
            self.end_tip(&mut tip, species);
        }
        self.tips[ti] = tip;
    }

    /// Vines look for a woody neighbor anywhere in the surrounding area and
    /// coil up it; with nothing to climb they creep sideways along the ground.
    fn steer_climb(&mut self, tip: &mut Tip, species: &Species, world: &World) -> bool {
        let f = species.form;
        if tip.support.is_none() {
            let search = f.climb_search.min(self.limits.max_radius_cells);
            if let Some(found) = world.find_support(self.col, self.row, search, &SUPPORT_LAYERS) {
                if found.owner != self.id {
                    tip.support = Some(found);
                }
            }
        }
        let support = match tip.support {
            Some(s) => s,
            None => {
                let target = if tip.dir > 0.0 { 0.0 } else { std::f64::consts::PI };
                tip.angle += angle_diff(target, tip.angle) * 0.35;
                return false;
            }
        };
        tip.phase += f.wrap_pitch;
        // Supports are found anywhere in the area but climbed on screen, so
        // only the horizontal offset steers the tip; the depth offset is left
        // alone.
        let target_x = self.ox as f64 + (support.col - self.col) as f64 * self.cell_px as f64;
        let pull = clamp((target_x - tip.x) * 0.05, -0.7, 0.7);
        let sway = tip.phase.sin() * to_rad(f.wrap_amp);
        let desired = -std::f64::consts::FRAC_PI_2 + sway + pull;
        tip.angle += angle_diff(desired, tip.angle) * 0.55;
        tip.phase.cos() < 0.0
    }

    fn branch(&mut self, tip: &mut Tip, species: &Species) {
        let f = species.form;
        let side = self.rng.sign();
        let angle = to_rad(self.rng.range(f.branch_angle_min, f.branch_angle_max)) * side;
        self.tips.push(Tip {
            x: tip.x,
            y: tip.y,
            angle: tip.angle + angle,
            width: (tip.width * 0.72).max(f.min_width),
            depth: tip.depth + 1,
            len: 0.0,
            since_branch: 0.0,
            phase: tip.phase + std::f64::consts::FRAC_PI_2,
            dir: -tip.dir,
            support: tip.support,
            alive: true,
        });
        self.alive_tips += 1;
        tip.angle -= angle * 0.35;
        tip.since_branch = 0.0;
        tip.width *= 0.94;
    }

    fn end_tip(&mut self, tip: &mut Tip, species: &Species) {
        if !tip.alive {
            return;
        }
        tip.alive = false;
        self.alive_tips -= 1;
        let f = species.form;
        if f.leaf_density > 0.0 && tip.depth >= f.leaf_depth {
            self.add_leaf(tip, false, species);
        }
    }

    fn add_leaf(&mut self, tip: &Tip, behind: bool, species: &Species) {
        let f = species.form;
        let r = self.rng.range(f.leaf_size_min, f.leaf_size_max);
        let side = self.rng.sign();
        let off = f.petiole + r * 0.5;
        let a = tip.angle + side * to_rad(self.rng.range(30.0, 80.0));
        let lx = tip.x + a.cos() * off;
        let ly = tip.y + a.sin() * off;
        if (lx - self.ox as f64).abs() > self.max_radius_px - r || ly < r || ly > self.oy as f64 {
            return;
        }
        let bias = if behind {
            -((species.shade.behind_shade * 100.0).round() as i8)
        } else {
            0
        };
        if f.petiole > 0.0 {
            self.segments.push(Segment {
                x0: tip.x,
                y0: tip.y,
                x1: lx - a.cos() * r * 0.4,
                y1: ly - a.sin() * r * 0.4,
                w: 1.0,
                mat: Mat::Stem,
                bias,
            });
        }
        let rx = r * self.rng.range(0.9, 1.35);
        let ry = r * self.rng.range(0.7, 1.1);
        let seed = self.rng.seed();
        self.leaves.push(Leaf { x: lx, y: ly, rx, ry, seed, bias });
        self.radius_px = self.radius_px.max((lx - self.ox as f64).abs() + r);
        self.height_px = self.height_px.max(self.oy as f64 - (ly - r));
    }

    /// Asks the world for a larger footprint on the ground plane. Returns false
    /// when a neighbor of the same size class already owns one of the cells.
    fn request_space(&mut self, radius_cells: i32, world: &mut World, blocked: Option<&[u8]>) -> bool {
        let mut cells = Vec::new();
        world.footprint(self.col, self.row, radius_cells, &mut cells);
        if !world.can_claim(self.layer, &cells, self.id) {
            return false;
        }
        if let Some(blocked) = blocked {
            if cells.iter().any(|&i| blocked[i] != 0) {
                return false;
            }
        }
        world.release(self.layer, &self.cells, self.id);
        world.claim(self.layer, &cells, self.id);
        self.cells = cells;
        self.granted_radius_cells = self.granted_radius_cells.max(radius_cells);
        true
    }

    // ---- rasterizing -----------------------------------------------------

    /// Widens the stamped rectangle to cover a stamp's clipped extent. The
    /// extent may be conservative: what matters is that nothing is ever
    /// written outside it.
    fn mark_stamped(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        if x1 < x0 || y1 < y0 {
            return;
        }
        if self.stamped.is_empty() {
            self.stamped = Bounds { x0, y0, x1, y1 };
        } else {
            let s = &mut self.stamped;
            s.x0 = s.x0.min(x0);
            s.y0 = s.y0.min(y0);
            s.x1 = s.x1.max(x1);
            s.y1 = s.y1.max(y1);
        }
    }

    /// Stamps one disc of a segment into the wood plane.
    fn stamp_disc(&mut self, cx: f64, cy: f64, r: f64, mat: Mat, bias: i8) {
        let rr = r.max(0.5);
        let x0 = ((cx - rr).floor() as i32).max(0);
        let x1 = ((cx + rr).ceil() as i32).min(self.w - 1);
        let y0 = ((cy - rr).floor() as i32).max(0);
        let y1 = ((cy + rr).ceil() as i32).min(self.h - 1);
        self.mark_stamped(x0, y0, x1, y1);
        let r2 = rr * rr;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f64 + 0.5 - cx;
                let dy = y as f64 + 0.5 - cy;
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                let i = (y * self.w + x) as usize;
                self.wood_mask[i] = mat as u8;
                self.wood_bias[i] = bias;
            }
        }
    }

    fn stamp_segment(&mut self, seg: Segment) {
        let dx = seg.x1 - seg.x0;
        let dy = seg.y1 - seg.y0;
        let len = dx.hypot(dy);
        let steps = ((len * 2.0).ceil() as i32).max(1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            self.stamp_disc(seg.x0 + dx * t, seg.y0 + dy * t, seg.w / 2.0, seg.mat, seg.bias);
        }
    }

    fn stamp_leaf(&mut self, leaf: Leaf) {
        let x0 = ((leaf.x - leaf.rx - 1.0).floor() as i32).max(0);
        let x1 = ((leaf.x + leaf.rx + 1.0).ceil() as i32).min(self.w - 1);
        let y0 = ((leaf.y - leaf.ry - 1.0).floor() as i32).max(0);
        let y1 = ((leaf.y + leaf.ry + 1.0).ceil() as i32).min(self.h - 1);
        self.mark_stamped(x0, y0, x1, y1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = (x as f64 + 0.5 - leaf.x) / leaf.rx.max(0.5);
                let dy = (y as f64 + 0.5 - leaf.y) / leaf.ry.max(0.5);
                let d = (dx * dx + dy * dy).sqrt();
                let wobble = (hash2(x, y, leaf.seed as i32) - 0.5) * 0.45;
                if d > 1.0 + wobble {
                    continue;
                }
                let i = (y * self.w + x) as usize;
                self.leaf_mask[i] = Mat::Leaf as u8;
                self.leaf_bias[i] = leaf.bias;
            }
        }
    }

    /// A mat is a ragged disc lying on the ground plane, squashed by the depth
    /// ratio, plus a short lip along its front edge so it reads as raised.
    fn stamp_ground_patch(&mut self) {
        let rx = self.radius_px.max(1.0);
        let ry = (rx * self.depth_ratio).max(1.0);
        let lip = (self.height_px * 0.5).round().max(0.0) as i32;
        let x0 = ((self.ox as f64 - rx - 1.0).floor() as i32).max(0);
        let x1 = ((self.ox as f64 + rx + 1.0).ceil() as i32).min(self.w - 1);
        let y0 = ((self.oy as f64 - ry - 1.0).floor() as i32).max(0);
        let y1 = ((self.oy as f64 + ry + 1.0).ceil() as i32).min(self.h - 1);
        // The lip below the disc reaches at most `lip` rows past the ellipse.
        self.mark_stamped(x0, y0, x1, (y1 + lip).min(self.h - 1));
        let mut bottom = vec![-1i32; self.w as usize];
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = (x as f64 + 0.5 - self.ox as f64) / rx;
                let dy = (y as f64 + 0.5 - self.oy as f64) / ry;
                let d = (dx * dx + dy * dy).sqrt();
                let wobble = (hash2(x, y, self.seed as i32) - 0.5) * 0.4;
                if d > 1.0 + wobble {
                    continue;
                }
                self.mask[(y * self.w + x) as usize] = Mat::Ground as u8;
                if y > bottom[x as usize] {
                    bottom[x as usize] = y;
                }
            }
        }
        for x in x0..=x1 {
            if bottom[x as usize] < 0 {
                continue;
            }
            let thick = (lip as f64 * (0.6 + 0.4 * hash2(x, 1, self.seed as i32))).round() as i32;
            for k in 1..=thick {
                let y = bottom[x as usize] + k;
                if y >= self.h {
                    break;
                }
                self.mask[(y * self.w + x) as usize] = Mat::Ground as u8;
            }
        }
    }

    /// Turns the rim of every leaf blob into leaf edge. `rect` is where the
    /// foliage is; nothing outside it is a leaf.
    fn mark_leaf_edges(&mut self, rect: Bounds) {
        let (w, h) = (self.w, self.h);
        if rect.is_empty() {
            return;
        }
        let mut edges = Vec::new();
        for y in rect.y0..=rect.y1 {
            for x in rect.x0..=rect.x1 {
                let i = (y * w + x) as usize;
                if self.mask[i] != Mat::Leaf as u8 {
                    continue;
                }
                let leaf = Mat::Leaf as u8;
                let up = if y > 0 { self.mask[i - w as usize] } else { 0 };
                let dn = if y < h - 1 { self.mask[i + w as usize] } else { 0 };
                let lf = if x > 0 { self.mask[i - 1] } else { 0 };
                let rt = if x < w - 1 { self.mask[i + 1] } else { 0 };
                if up != leaf || dn != leaf || lf != leaf || rt != leaf {
                    edges.push(i);
                }
            }
        }
        for i in edges {
            self.mask[i] = Mat::LeafEdge as u8;
        }
    }

    /// Gives a plant read back off a save the pixel buffers it was written
    /// without, ready for the next pass of the raster queue to fill them. The
    /// sizes come from `w` and `h`, which are fixed when the plant is created.
    pub fn rehydrate(&mut self) {
        let n = (self.w.max(0) * self.h.max(0)) as usize;
        self.mask = vec![Mat::Empty as u8; n];
        self.bias = vec![0; n];
        self.sprite = vec![EMPTY_COLOR; n];
        self.stamped = Bounds::default();
        self.wood_mask = Vec::new();
        self.wood_bias = Vec::new();
        self.leaf_mask = Vec::new();
        self.leaf_bias = Vec::new();
        self.stamped_segments = 0;
        self.stamped_leaves = 0;
        self.alive_tips = self.tips.iter().filter(|t| t.alive).count() as i32;
        self.dirty = true;
    }

    pub fn raster(&mut self, env: &RasterEnv, scratch: &mut Scratch, species: &Species) {
        let group_bounds;
        if self.size_class == SizeClass::Ground {
            // A patch is one disc whose radius moves, so it is drawn whole
            // every time. Only the rows the last raster stamped need clearing:
            // the rest of the buffer has been empty since the plant was made
            // or rehydrated.
            let prev = self.stamped;
            if !prev.is_empty() {
                let w = self.w;
                for y in prev.y0..=prev.y1 {
                    let a = (y * w + prev.x0) as usize;
                    let b = (y * w + prev.x1 + 1) as usize;
                    self.mask[a..b].fill(Mat::Empty as u8);
                    self.bias[a..b].fill(0);
                    self.sprite[a..b].fill(EMPTY_COLOR);
                }
            }
            self.stamped = Bounds::default();
            self.stamp_ground_patch();
            group_bounds = [Bounds::default(), Bounds::default(), self.stamped];
        } else {
            // Stamp only what was appended since the last raster, then build
            // the working mask by laying the leaf plane over the wood plane,
            // which is the same order a full re-stamp would have drawn them
            // in. Everything a shrivel ate out of the working mask comes back
            // here and is eaten again a little further on, exactly as a full
            // re-stamp would have redrawn it.
            let n = (self.w.max(0) * self.h.max(0)) as usize;
            if self.wood_mask.len() != n
                || self.segments.len() < self.stamped_segments
                || self.leaves.len() < self.stamped_leaves
            {
                self.wood_mask = vec![0; n];
                self.wood_bias = vec![0; n];
                self.leaf_mask = vec![0; n];
                self.leaf_bias = vec![0; n];
                self.stamped_segments = 0;
                self.stamped_leaves = 0;
            }
            let segments = std::mem::take(&mut self.segments);
            for seg in &segments[self.stamped_segments..] {
                self.stamp_segment(*seg);
            }
            self.stamped_segments = segments.len();
            self.segments = segments;
            let leaves = std::mem::take(&mut self.leaves);
            for leaf in &leaves[self.stamped_leaves..] {
                self.stamp_leaf(*leaf);
            }
            self.stamped_leaves = leaves.len();
            self.leaves = leaves;

            let sb = self.stamped;
            let mut gb = [Bounds::default(); 3];
            if !sb.is_empty() {
                let w = self.w;
                for y in sb.y0..=sb.y1 {
                    for x in sb.x0..=sb.x1 {
                        let i = (y * w + x) as usize;
                        let lm = self.leaf_mask[i];
                        let m = if lm != 0 {
                            self.bias[i] = self.leaf_bias[i];
                            lm
                        } else {
                            self.bias[i] = self.wood_bias[i];
                            self.wood_mask[i]
                        };
                        self.mask[i] = m;
                        if m != 0 {
                            gb[GROUP_OF[m as usize]].include(x, y);
                        }
                    }
                }
            }
            if species.form.leaf_edges {
                self.mark_leaf_edges(gb[1]);
            }
            group_bounds = gb;
        }

        self.shade(env, scratch, species, &group_bounds);
        if self.wither > 0.0 {
            self.dry_out();
            // Drying out eats pixels, so only a scan can say what is left.
            self.update_bounds();
        } else if self.size_class == SizeClass::Ground {
            // The patch's rectangle is the stamp's reach, not what the wobble
            // kept, so the drawn box still has to be measured.
            self.update_bounds();
        } else {
            // Nothing was eaten, and the group boxes were measured off every
            // drawn pixel while the mask was composited: their union is the
            // drawn box.
            let mut b = Bounds { x0: self.w, y0: self.h, x1: -1, y1: -1 };
            for gb in &group_bounds {
                if gb.is_empty() {
                    continue;
                }
                b.include(gb.x0, gb.y0);
                b.include(gb.x1, gb.y1);
            }
            self.bounds = b;
        }
        self.update_tint();
        self.dirty = false;
    }

    /// Past its age, a plant dries out rather than blinking off the map. It is
    /// only re-drawn every so often through this: a shrivel is a handful of
    /// visible steps, not one per frame, and re-rastering is the expensive
    /// part of the whole simulation.
    fn shrivel(&mut self, dt: f64, seconds: f64) {
        let before = (self.wither * SHRIVEL_STEPS).floor();
        self.wither += dt / seconds.max(0.05);
        if (self.wither * SHRIVEL_STEPS).floor() != before {
            self.dirty = true;
        }
        if self.wither >= 1.0 {
            self.wither = 1.0;
            self.alive = false;
        }
    }

    /// Browns what is drawn and takes it apart from the tips down: the top of
    /// a plant is the thin end and goes first, and a little noise per pixel
    /// keeps the edge ragged rather than a line sweeping down the sprite.
    fn dry_out(&mut self) {
        let t = clamp01(self.wither);
        let sb = self.stamped;
        if sb.is_empty() {
            return;
        }
        // Read off the mask rather than the stored bounds: every raster stamps
        // the whole plant again and this eats into it afterwards, so the
        // bounds still describe the last drawing, not this one.
        let (mut top, mut base) = (self.h, -1);
        for y in sb.y0..=sb.y1 {
            let row = (y * self.w + sb.x0) as usize;
            let n = (sb.x1 - sb.x0 + 1) as usize;
            if self.mask[row..row + n].iter().any(|m| *m != 0) {
                if y < top {
                    top = y;
                }
                base = y;
            }
        }
        if base < top {
            return;
        }
        let span = (base - top).max(1) as f64;
        let dead = unpack_rgba(DEAD_COLOR);
        for y in sb.y0..=sb.y1 {
            // 1 at the tips, 0 at the foot.
            let height = ((base - y) as f64 / span).clamp(0.0, 1.0);
            for x in sb.x0..=sb.x1 {
                let i = (y * self.w + x) as usize;
                if self.mask[i] == 0 {
                    continue;
                }
                let noise = hash2(x, y, self.seed as i32);
                // The tips let go early, the foot hangs on to the end.
                let gone = 0.12 + 0.72 * (1.0 - height) + 0.16 * noise;
                if t >= gone {
                    self.mask[i] = Mat::Empty as u8;
                    self.sprite[i] = EMPTY_COLOR;
                    continue;
                }
                // Browning runs ahead of the falling apart, so a plant is
                // visibly dead before it starts to go.
                let c = unpack_rgba(self.sprite[i]);
                let k = clamp01(t * 1.6);
                let mix = |from: u8, to: u8| (from as f64 + (to as f64 - from as f64) * k) as i32;
                self.sprite[i] =
                    pack_rgba(mix(c.r, dead.r), mix(c.g, dead.g), mix(c.b, dead.b), c.a as i32);
            }
        }
    }

    fn shade(
        &mut self,
        env: &RasterEnv,
        sc: &mut Scratch,
        species: &Species,
        group_bounds: &[Bounds; 3],
    ) {
        // Everything a group draws sits inside that group's rectangle, so the
        // group mask, the distance transform and the component labels are
        // computed over that rectangle alone, in its own coordinates. The
        // distance transform treats the edge of its buffer as background,
        // which is exactly what surrounds the rectangle in the full buffer.
        let w = self.w as usize;
        let tones = species.shade.tones;
        let jitter = species.shade.jitter;

        for (gi, group) in SHADE_GROUPS.iter().enumerate() {
            // Each group works inside its own rectangle, found while the mask
            // was being built: the trunk pass does not sweep the crown and the
            // leaf pass does not sweep the trunk. An empty rectangle is a
            // group with nothing in it.
            let gb = group_bounds[gi];
            if gb.is_empty() {
                continue;
            }
            let bw = (gb.x1 - gb.x0 + 1) as usize;
            let bh = (gb.y1 - gb.y0 + 1) as usize;
            let bits: u32 = group.mats.iter().fold(0, |a, m| a | 1 << (*m as u8));
            sc.gmask.clear();
            sc.gmask.resize(bw * bh, 0);
            for ly in 0..bh {
                let row = (gb.y0 as usize + ly) * w + gb.x0 as usize;
                for lx in 0..bw {
                    let m = self.mask[row + lx];
                    sc.gmask[ly * bw + lx] = (bits & (1u32 << m) != 0) as u8;
                }
            }
            distance_transform(&sc.gmask, bw, bh, &mut sc.dist);
            let comps =
                label_components(&sc.gmask, bw, bh, &sc.dist, &mut sc.labels, &mut sc.stack);

            let core = species.core_for(group.wood).max(0.5);
            let adaptive = species.shade.adaptive_core;
            // Fixed core depth keeps thin twigs light and only lets thick
            // bodies reach the darkest tone; adaptive rescales per shape so
            // every shape uses the full ramp.
            let norms: Vec<f64> = comps
                .iter()
                .map(|c| {
                    if adaptive {
                        core.min((c.max_depth as f64).max(0.5))
                    } else {
                        core
                    }
                })
                .collect();
            // The two vertical curve terms depend only on the component and
            // the row, so they are worked out once per component per row
            // rather than once per pixel. The stamp says which row an entry
            // was cached on.
            sc.vcache.clear();
            sc.vcache.resize(comps.len(), (0.0, 0.0, 0.0));
            sc.vstamp.clear();
            sc.vstamp.resize(comps.len(), 0);
            let shading = env.shading;
            for ly in 0..bh {
                for lx in 0..bw {
                    let li = ly * bw + lx;
                    let l = sc.labels[li];
                    if l < 0 {
                        continue;
                    }
                    let lu = l as usize;
                    let (x, y) = (gb.x0 as usize + lx, gb.y0 as usize + ly);
                    let i = y * w + x;
                    if sc.vstamp[lu] != ly as u32 + 1 {
                        sc.vstamp[lu] = ly as u32 + 1;
                        let comp = comps[lu];
                        // The component box is in rectangle coordinates, and
                        // so is ly here: the offset cancels out of the
                        // fraction.
                        let span = comp.y1 - comp.y0;
                        let vert = if span > 0 {
                            (ly as i32 - comp.y0) as f64 / span as f64
                        } else {
                            0.0
                        };
                        let up = shading.top_light * curve_value(1.0 - vert, shading);
                        let down = shading.bottom_dark * curve_value(vert, shading);
                        sc.vcache[lu] = (vert, up, down);
                    }
                    let (vert, up, down) = sc.vcache[lu];
                    let nd = clamp01(sc.dist[li] as f64 / norms[lu]);
                    let mut t = shading.mid;
                    t -= shading.center_dark * curve_value(nd, shading);
                    t += up;
                    t -= down;
                    t = t.clamp(0.0, 1.0);
                    t += self.bias[i] as f64 / 100.0 + self.depth_shade;
                    if jitter > 0.0 {
                        t += (hash2(x as i32, y as i32, self.seed as i32) - 0.5) * 2.0 * jitter;
                    }
                    let q = quantize(clamp01(t), tones);
                    let ramp = &env.ramps[self.mask[i] as usize];
                    if !ramp.is_empty() {
                        // How far down the shape this pixel is chooses which
                        // part of the box it reads, so a box drawn with a light
                        // crown and a dark base comes out that way round.
                        self.sprite[i] = ramp.pick(q, vert);
                    } else {
                        // The sprite is not cleared between rasters, so a
                        // material whose box is empty has to write the empty
                        // pixel a clear would have left.
                        self.sprite[i] = EMPTY_COLOR;
                    }
                }
            }
        }
    }

    fn update_bounds(&mut self) {
        let mut b = Bounds { x0: self.w, y0: self.h, x1: -1, y1: -1 };
        let sb = self.stamped;
        for y in sb.y0..=sb.y1 {
            for x in sb.x0..=sb.x1 {
                if self.mask[(y * self.w + x) as usize] == 0 {
                    continue;
                }
                if x < b.x0 {
                    b.x0 = x;
                }
                if x > b.x1 {
                    b.x1 = x;
                }
                if y < b.y0 {
                    b.y0 = y;
                }
                if y > b.y1 {
                    b.y1 = y;
                }
            }
        }
        self.bounds = b;
    }

    /// The mean of every drawn pixel. Sampled on a stride rather than over the
    /// whole sprite, because it is only ever used as one pixel on screen.
    fn update_tint(&mut self) {
        let b = self.bounds;
        if b.is_empty() {
            self.tint = 0;
            return;
        }
        let (mut r, mut g, mut bl, mut n) = (0u32, 0u32, 0u32, 0u32);
        let step = (((b.x1 - b.x0 + 1) * (b.y1 - b.y0 + 1)) / 256).max(1);
        let mut k = 0;
        for y in b.y0..=b.y1 {
            for x in b.x0..=b.x1 {
                k += 1;
                if k % step != 0 {
                    continue;
                }
                let v = self.sprite[(y * self.w + x) as usize];
                if v == 0 {
                    continue;
                }
                let c = unpack_rgba(v);
                r += c.r as u32;
                g += c.g as u32;
                bl += c.b as u32;
                n += 1;
            }
        }
        self.tint = match r.checked_div(n) {
            Some(r) => pack_rgba(r as i32, (g / n) as i32, (bl / n) as i32, 255),
            // Nothing drawn, so there is no color to stand for it.
            None => 0,
        };
    }
}
