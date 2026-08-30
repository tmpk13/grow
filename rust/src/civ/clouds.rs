//! Clouds passing over the settlement. One seamless tile of them is generated
//! from wrapped value noise and read twice: stamped over the sky band of the
//! frame buffer, under everything that stands up into the sky, and - when the
//! switch for it is on - repeated across the empty space around the map by the
//! camera.
//!
//! Everything here runs on simulation time, like the wind: two runs of one
//! seed are the same sky, a restored settlement picks up the clouds it saved
//! under, and a paused world holds its weather still. The shapes drift whole
//! by an offset the tile does not know about; what the tile itself animates is
//! the edges, which churn at a low, settable amplitude - clouds boiling
//! slowly rather than sliding as one rigid picture.

use crate::state::State;
use crate::util::{hex_to_packed, mix_packed, pack_rgba};

/// The tile. Sized so the lattice of both octaves wraps exactly, which is what
/// makes it seamless in both directions, and wide enough that the repeat is
/// not read as a repeat: a distinctive shape recurring every couple of hundred
/// pixels is the first thing an eye finds.
pub const TILE_W: i32 = 384;
pub const TILE_H: i32 = 192;

/// How many times a second the edges take a step. The amplitude is settable;
/// the rate is not, because below this it reads as broken rather than slow and
/// above it as static on the sky.
const WOBBLE_HZ: f64 = 4.0;

/// The current tile and where it has drifted to. Rebuilt from the simulation
/// clock, never saved: a restored settlement regenerates the same sky.
#[derive(Default)]
pub struct CloudLayer {
    pub w: i32,
    pub h: i32,
    /// Packed pixels, zero where the sky shows through.
    pub px: Vec<u32>,
    /// What the tile was built from, so a frame that changed nothing reuses
    /// it. Doubles as the camera's key for knowing when to re-upload.
    pub key: u64,
    /// Whole world pixels the field has drifted, applied by both readers.
    pub drift: i32,
    /// The field the pixels were colored from, kept for the underside pass.
    scratch: Vec<f32>,
}

/// One octave's lattice, its corner values worked out once per rebuild. At
/// full speed the wobble step moves every frame and the whole tile is redrawn
/// with it, so the per pixel work has to be a couple of lerps into this
/// rather than a fistful of hashes.
struct Lattice {
    v: Vec<f64>,
    nx: i32,
    ny: i32,
    cell: f64,
}

impl Lattice {
    /// Corner values with the wobble already in them: base per point, plus a
    /// churn whose phase belongs to the point, so the edges boil without the
    /// shapes moving as one.
    fn new(cell: i32, seed: i32, t: f64, wobble: f64) -> Lattice {
        let nx = TILE_W / cell;
        // The vertical axis is squashed: a cloud is wider than it is tall.
        // The stretch is an integer so the lattice still wraps exactly at the
        // tile's height.
        let ny = TILE_H * VERTICAL_SQUASH / cell;
        let mut v = vec![0.0; (nx * ny) as usize];
        for yi in 0..ny {
            for xi in 0..nx {
                let base = crate::util::hash2(xi, yi, seed);
                v[(yi * nx + xi) as usize] = if wobble <= 0.0 {
                    base
                } else {
                    let phase =
                        crate::util::hash2(xi, yi, seed ^ 0x5bd1) * std::f64::consts::TAU;
                    base + wobble * 0.38 * (t + phase).sin()
                };
            }
        }
        Lattice { v, nx, ny, cell: cell as f64 }
    }

    fn at(&self, x: f64, y: f64) -> f64 {
        let gx = x / self.cell;
        let gy = y / self.cell;
        let x0 = gx.floor();
        let y0 = gy.floor();
        let fx = smooth(gx - x0);
        let fy = smooth(gy - y0);
        let corner = |dx: i32, dy: i32| -> f64 {
            let xi = (x0 as i32 + dx).rem_euclid(self.nx);
            let yi = (y0 as i32 + dy).rem_euclid(self.ny);
            self.v[(yi * self.nx + xi) as usize]
        };
        let top = corner(0, 0) * (1.0 - fx) + corner(1, 0) * fx;
        let bottom = corner(0, 1) * (1.0 - fx) + corner(1, 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// How much wider than tall a cloud is drawn.
const VERTICAL_SQUASH: i32 = 2;

/// The two octaves a tile is built from. Cells divide the tile exactly: 384
/// and 192 by 48 and by 16. The broad octave is what makes a mass rather than
/// popcorn; the fine one frays it.
fn lattices(seed: i32, t: f64, wobble: f64) -> (Lattice, Lattice) {
    (Lattice::new(48, seed, t, wobble), Lattice::new(16, seed ^ 0x9e37, t * 1.7, wobble))
}

fn sample(broad: &Lattice, fine: &Lattice, x: i32, y: i32) -> f64 {
    let (xf, yf) = (x as f64, (y * VERTICAL_SQUASH) as f64);
    broad.at(xf, yf) * 0.62 + fine.at(xf, yf) * 0.38
}

/// Public for the tests, which check the tile is seamless where it wraps.
/// Builds the lattices per call; the rebuild inside `refresh` builds them once
/// and samples the same way.
pub fn field(x: i32, y: i32, seed: i32, t: f64, wobble: f64) -> f64 {
    let (broad, fine) = lattices(seed, t, wobble);
    sample(&broad, &fine, x, y)
}

/// Brings the layer up to the moment: the drift every frame, the tile itself
/// only when the quantized wobble step or a parameter has moved. With the
/// switch off the layer empties, which is also what tells the camera there is
/// nothing to repeat over the empty space.
pub fn refresh(layer: &mut CloudLayer, state: &State, time: f64) {
    let view = &state.civ.view;
    if !view.clouds {
        layer.px.clear();
        layer.key = 0;
        return;
    }
    layer.drift = (time * view.cloud_speed).round() as i32;

    let wobble = view.cloud_wobble.clamp(0.0, 1.0);
    // With no wobble the shapes never change, so time leaves the key alone
    // and the tile is built once.
    let step = if wobble > 0.0 { (time * WOBBLE_HZ).floor() as i64 } else { 0 };
    let key = mix_key(
        state.civ.seed as u64,
        step as u64,
        (view.cloud_cover * 1000.0) as u64,
        (wobble * 1000.0) as u64,
        hex_to_packed(&state.civ.world.sky_top) as u64,
    );
    if key == layer.key && !layer.px.is_empty() {
        return;
    }
    layer.key = key;
    layer.w = TILE_W;
    layer.h = TILE_H;

    let seed = state.civ.seed as i32;
    let t = step as f64 * (std::f64::consts::TAU / (WOBBLE_HZ * 6.0));
    let n = (TILE_W * TILE_H) as usize;
    let (broad, fine) = lattices(seed, t, wobble);
    layer.scratch.resize(n, 0.0);
    for y in 0..TILE_H {
        for x in 0..TILE_W {
            layer.scratch[(y * TILE_W + x) as usize] = sample(&broad, &fine, x, y) as f32;
        }
    }

    // The palette leans on the sky it hangs in, so recoloring the sky
    // recolors the weather.
    let sky_top = hex_to_packed(&state.civ.world.sky_top);
    let sky_bottom = hex_to_packed(&state.civ.world.sky_bottom);
    let white = pack_rgba(236, 242, 248, 255);
    let core = mix_packed(white, sky_top, 0.08);
    let body = mix_packed(white, sky_top, 0.24);
    let under = mix_packed(white, sky_bottom, 0.48);

    let threshold = 0.86 - view.cloud_cover.clamp(0.0, 1.0) * 0.52;
    layer.px.clear();
    layer.px.resize(n, 0);
    for y in 0..TILE_H {
        for x in 0..TILE_W {
            let i = (y * TILE_W + x) as usize;
            let d = layer.scratch[i] as f64 - threshold;
            if d < 0.0 {
                continue;
            }
            // A ragged pixel edge rather than a hard contour.
            if d < 0.045 && crate::util::hash2(x, y, seed ^ 0x2f1) > d / 0.045 {
                continue;
            }
            // The bottom of a shape is in shade; the thick of it is brightest.
            let below = (y + 3).rem_euclid(TILE_H);
            let thins_below = (layer.scratch[(below * TILE_W + x) as usize] as f64) < threshold + 0.02;
            layer.px[i] = if thins_below {
                under
            } else if d > 0.16 {
                core
            } else {
                body
            };
        }
    }
}

fn mix_key(a: u64, b: u64, c: u64, d: u64, e: u64) -> u64 {
    let mut k = a ^ 0x9e3779b97f4a7c15;
    for v in [b, c, d, e] {
        k = (k ^ v).wrapping_mul(0xff51afd7ed558ccd);
        k ^= k >> 33;
    }
    // Zero is the empty layer's key, so a real tile never claims it.
    k.max(1)
}
