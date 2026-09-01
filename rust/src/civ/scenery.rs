//! What stands behind the map: hills and mountains in the sky band.
//!
//! The midground is scenery and nothing else. It is drawn into the cached
//! ground, above the sky and before the land, so the map overdraws its foot and
//! it reads as standing beyond the far edge; nothing walks on it, nothing is
//! built on it, and the simulation never asks about it.
//!
//! A piece is a shape, a width and a height in cells, and how far off it is.
//! Distance is the one number that does two things: it hazes the piece toward
//! the sky and it decides what stands in front of what, because the further
//! thing is always the one behind.

use serde::{Deserialize, Serialize};

use crate::civ::settlement::Settlement;
use crate::sampler::Bands;
use crate::state::State;
use crate::util::{clamp01, hash2, mix_packed};
use crate::world::World;

/// The three shapes a piece takes. Anything else somebody wants is these three
/// at different widths, standing beside each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shape {
    /// A peak: steep, one summit, and snow on it if it is high enough.
    Peak,
    /// A ridge: a long line of smaller summits, flat topped as a whole.
    Ridge,
    /// A bank: a rounded rise, which is what a hill is at this distance.
    Bank,
}

pub const SHAPES: [Shape; 3] = [Shape::Peak, Shape::Ridge, Shape::Bank];

impl Shape {
    pub fn label(self) -> &'static str {
        match self {
            Shape::Peak => "Mountain",
            Shape::Ridge => "Ridge",
            Shape::Bank => "Hill",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Shape::Peak => "peak",
            Shape::Ridge => "ridge",
            Shape::Bank => "bank",
        }
    }

    pub fn from_key(key: &str) -> Shape {
        SHAPES.iter().copied().find(|s| s.key() == key).unwrap_or(Shape::Peak)
    }
}

/// One thing standing behind the map.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Scene {
    pub shape: Shape,
    /// Where its middle stands, in cells across the map. Outside the map is
    /// allowed: the sky runs past the land on both sides.
    pub x: f64,
    /// How wide and how tall, in cells, so a piece keeps its size against the
    /// map however large a cell is drawn.
    pub width: f64,
    pub height: f64,
    /// How far off it is, from 0 for the near ridge to 1 for something barely
    /// there. Hazes it into the sky and puts it behind everything nearer.
    pub distance: f64,
    /// Where the snow starts, as a share of the height, or 1 for none.
    pub snow: f64,
    /// The sampling box its rock is read from.
    pub sampler: String,
    /// What makes one mountain a different mountain from the next of the same
    /// shape: every wobble in its outline is hashed from this.
    pub seed: u32,
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            shape: Shape::Peak,
            x: 0.0,
            width: 26.0,
            height: 9.0,
            distance: 0.55,
            snow: 0.72,
            sampler: "mat-stone".to_string(),
            seed: 1,
        }
    }
}

impl Scene {
    pub fn label(&self) -> String {
        format!("{} at {:.0}", self.shape.label(), self.x)
    }

    /// The outline, as a share of the height, at `t` across the piece. The same
    /// hash is asked the same questions in the same order every time, so a
    /// piece is the same picture in every frame it is drawn in.
    pub fn profile(&self, t: f64) -> f64 {
        if !(0.0..=1.0).contains(&t) {
            return 0.0;
        }
        let seed = self.seed as i32;
        let wobble = |k: i32, at: f64| (hash2(k, (at * 64.0) as i32, seed) - 0.5) * 2.0;
        let d = (t * 2.0 - 1.0).abs();
        match self.shape {
            Shape::Peak => {
                // Steep sides that ease off toward the summit, with the summit
                // itself off center and one shoulder lower than the other.
                let lean = (hash2(1, 1, seed) - 0.5) * 0.5;
                let m = ((t - (0.5 + lean * 0.3)) * 2.0).abs().min(1.0);
                let base = (1.0 - m.powf(1.45)).max(0.0);
                let shoulders = 0.90 + 0.10 * ((t * 9.0 + lean * 6.0).sin());
                clamp01(base * shoulders + wobble(2, t) * 0.02)
            }
            Shape::Ridge => {
                // Flat as a whole and broken along the top, tapering to nothing
                // at both ends rather than being cut off.
                let window = (1.0 - d.powi(4)).max(0.0);
                let tops = 0.62
                    + 0.16 * ((t * 7.0 + seed as f64 * 0.37).sin())
                    + 0.10 * ((t * 19.0 + seed as f64 * 0.11).sin());
                clamp01(window * tops + wobble(3, t) * 0.02)
            }
            Shape::Bank => {
                let round = (1.0 - d * d).max(0.0).sqrt();
                clamp01(round * (0.92 + 0.08 * ((t * 5.0 + seed as f64).sin())))
            }
        }
    }

    /// How tall this piece is at a screen column, in pixels, and how far along
    /// its own width that column is. Nothing outside its span.
    fn column(&self, world: &World, x: i32) -> Option<(i32, f64)> {
        let cell = world.cell_px.max(1) as f64;
        let half = (self.width.max(0.5) * cell) / 2.0;
        let mid = self.x * cell;
        let t = (x as f64 - (mid - half)) / (half * 2.0);
        if !(0.0..=1.0).contains(&t) {
            return None;
        }
        let h = (self.height.max(0.1) * cell * self.profile(t)).round() as i32;
        Some((h, t))
    }
}

/// A short fingerprint of everything the drawing reads, so the cached
/// background knows when the scenery has moved. Everything in it is authored:
/// there is no clock and no seed of the world's in here, which is what makes
/// it stable across a reload.
pub fn stamp(scenery: &[Scene]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |v: u64| {
        for b in v.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    for piece in scenery {
        eat(piece.shape as u64);
        eat(piece.x.to_bits());
        eat(piece.width.to_bits());
        eat(piece.height.to_bits());
        eat(piece.distance.to_bits());
        eat(piece.snow.to_bits());
        eat(piece.seed as u64);
        for b in piece.sampler.as_bytes() {
            eat(*b as u64);
        }
    }
    h
}

/// The pieces in the order they are drawn: the furthest first, so a nearer one
/// stands in front of it. Distance is the whole of the ordering, which is what
/// keeps the list on the panel from being a stack anybody has to shuffle.
pub fn back_to_front(scenery: &[Scene]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scenery.len()).collect();
    order.sort_by(|&a, &b| {
        scenery[b]
            .distance
            .partial_cmp(&scenery[a].distance)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    order
}

/// Which piece is under a point on the screen, if any: the nearest one whose
/// outline covers it. What a press on the sky lands on.
pub fn at(scenery: &[Scene], world: &World, x: i32, y: i32) -> Option<usize> {
    if y >= world.sky_px {
        return None;
    }
    for &i in back_to_front(scenery).iter().rev() {
        let piece = &scenery[i];
        if let Some((h, _)) = piece.column(world, x) {
            if h > 0 && y >= world.sky_px - h {
                return Some(i);
            }
        }
    }
    None
}

/// Paints the midground into the cached ground, over the sky and under the
/// land. Called from `paint_terrain`, which is the one place the sky exists as
/// pixels rather than as two colors.
pub fn paint(sim: &Settlement, state: &State, buf: &mut [u32], sky: &dyn Fn(i32) -> u32) {
    let scenery = &state.civ.scenery;
    if scenery.is_empty() {
        return;
    }
    let world = sim.world();
    let horizon = world.sky_px;
    if horizon <= 0 {
        return;
    }
    for i in back_to_front(scenery) {
        let piece = &scenery[i];
        let ramp = crate::civ::civ_render::ramp_for(&state.materials, &piece.sampler);
        paint_one(piece, world, &ramp, buf, horizon, sky);
    }
}

fn paint_one(
    piece: &Scene,
    world: &World,
    ramp: &Bands,
    buf: &mut [u32],
    horizon: i32,
    sky: &dyn Fn(i32) -> u32,
) {
    let haze = clamp01(piece.distance);
    // The furthest things lose their shading as well as their color: what is
    // left of a mountain on the horizon is a shape.
    let contrast = 1.0 - haze * 0.65;
    let snow_at = clamp01(piece.snow);
    let snow_color = crate::util::pack_rgba(238, 244, 250, 255);
    let seed = piece.seed as i32;
    for x in 0..world.px_w {
        let (h, t) = match piece.column(world, x) {
            Some(col) => col,
            None => continue,
        };
        if h <= 0 {
            continue;
        }
        // The lit side is the one the ground gets its light from: rising to
        // the right is a slope facing away, falling is one facing into it. It
        // follows how steep the slope is rather than which way it points, or
        // the summit - where the sign flips - would be a crease down the
        // middle of the picture.
        let slope = piece.profile((t + 0.03).min(1.0)) - piece.profile((t - 0.03).max(0.0));
        let facing = (-slope * 1.8).clamp(-0.12, 0.12);
        let top = (horizon - h).max(0);
        for y in top..horizon {
            if y < 0 || y >= world.px_h {
                continue;
            }
            // How high up this piece the pixel is, which is what the rock is
            // shaded by and where the snow line is read against.
            let up = (horizon - y) as f64 / h.max(1) as f64;
            let noise = (hash2(x, y, seed + 17) - 0.5) * 0.16;
            let tone = clamp01(0.5 + (up - 0.5) * 0.55 * contrast + facing * contrast + noise);
            let mut c = ramp.pick(tone, 1.0 - up * 0.8);
            if up > snow_at && snow_at < 1.0 {
                // The snow line is not a straight cut: it follows the rock a
                // little, and thins out just below where it starts.
                let over = ((up - snow_at) / (1.0 - snow_at).max(0.05)).min(1.0);
                let take = clamp01(over * 1.6 + noise * 2.0);
                c = mix_packed(c, snow_color, take);
            }
            let i = (y * world.px_w + x) as usize;
            if i < buf.len() {
                buf[i] = mix_packed(c, sky(y), haze * 0.85);
            }
        }
    }
}
