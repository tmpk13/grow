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
//!
//! The weather has a base: a line across the sky that the middle of a cloud
//! stays above. It is not a cut. Where a cloud's middle sinks below the line
//! that cloud thins and goes, whole and on its own, so the ones just above
//! the line hang below it and the underside of the weather is ragged the way
//! a real one is rather than ruled. What thins a cloud is its own threshold
//! being raised, never a mask laid over it: the sky it gives back is bounded
//! by the shape's own contour, dithered like every other cloud edge, instead
//! of by the seam between one cloud and the next - which is a straight
//! diagonal line, and read as a triangle bitten out of the weather. The tile
//! carries what that needs, which cloud every pixel belongs to, and answers
//! for any base line, so moving the line rebuilds nothing.

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
    /// The tile as it is drawn across the band that straddles the cloud base.
    /// Row `e` of it is world row `base - h / 2 + e`, shaded against a
    /// threshold raised by how far the middle of each pixel's cloud has sunk
    /// below the base. Above the band the tile is `px` whole; below it there
    /// is nothing. Both readers go through `row_at`, which is what keeps the
    /// map and the space around it the same sky.
    pub edge: Vec<u32>,
    /// What the tile was built from, so a frame that changed nothing reuses
    /// it. Doubles as the camera's key for knowing when to re-upload.
    pub key: u64,
    /// Whole world pixels the field has drifted, applied by both readers.
    pub drift: i32,
    /// The field the pixels were colored from, kept for the underside pass.
    scratch: Vec<f32>,
    /// The broad octave alone, with no wobble in it, which is what a cloud's
    /// middle is found in.
    broad: Vec<f32>,
    /// What the middles were worked out for, so they are worked out once per
    /// seed rather than once per tile. Zero means never.
    sink_key: u64,
    /// Rows from each pixel to the middle of the cloud it belongs to,
    /// negative upward, softened across the seams between one cloud and the
    /// next.
    sink: Vec<f32>,
    /// Scratch for the smoothing pass, kept so it is allocated once.
    blur: Vec<f32>,
}

impl CloudLayer {
    /// The tile row that world row `y` reads, against a cloud base at world
    /// row `base`: the tile whole above the band round the base, the edge
    /// tile across it, and nothing below. Rows read from the base line, so the
    /// same shape is at the same place for every reader.
    pub fn row_at(&self, y: i32, base: i32) -> Option<&[u32]> {
        if self.px.is_empty() || self.w <= 0 || self.h <= 0 {
            return None;
        }
        let w = self.w as usize;
        let half = self.h / 2;
        let (rows, sy) = if y < base - half {
            (&self.px, (y - base).rem_euclid(self.h))
        } else if y < base + half {
            (&self.edge, y - (base - half))
        } else {
            return None;
        };
        let at = sy as usize * w;
        rows.get(at..at + w)
    }

    /// Rows from one tile pixel to the middle of the cloud it belongs to,
    /// negative upward. This is what the band reads to decide how far a cloud
    /// has sunk past the base, and it is smoothed across the seam between one
    /// cloud and the next rather than stepping at it. Public for the test
    /// that holds it to that, since a step here is a straight diagonal line
    /// of sky on the map.
    pub fn rise_at(&self, x: i32, y: i32) -> f64 {
        if self.w <= 0 || self.h <= 0 {
            return 0.0;
        }
        let i = (y.rem_euclid(self.h) * self.w + x.rem_euclid(self.w)) as usize;
        self.sink.get(i).copied().unwrap_or(0.0) as f64
    }
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

/// The two octaves mixed, at one pixel.
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

/// Which cloud every pixel belongs to, worked out once per seed. The wobble
/// is deliberately left out of the octave this climbs: what churns is the
/// edge of a shape, not where the middle of it is, so neither the climb nor
/// the smoothing after it has to be redone every time the tile is rebuilt -
/// which at any wobble at all is several times a second.
fn ensure_middles(layer: &mut CloudLayer, seed: i32) {
    let n = (TILE_W * TILE_H) as usize;
    // The low bit is set so a seed of zero is still a key that has been used.
    let key = (seed as u32 as u64) << 1 | 1;
    if layer.sink_key == key && layer.sink.len() == n {
        return;
    }
    layer.sink_key = key;
    let broad = Lattice::new(48, seed, 0.0, 0.0);
    layer.broad.clear();
    layer.broad.resize(n, 0.0);
    for y in 0..TILE_H {
        for x in 0..TILE_W {
            layer.broad[(y * TILE_W + x) as usize] =
                broad.at(x as f64, (y * VERTICAL_SQUASH) as f64) as f32;
        }
    }
    find_middles(&layer.broad, TILE_W, TILE_H, &mut layer.sink);
    blur_wrapped(&mut layer.sink, &mut layer.blur, TILE_W, TILE_H, SINK_BLUR);
}

/// Which cloud each pixel belongs to, as rows from the pixel to the middle
/// of it. A middle is a local top of the broad octave, every pixel climbs to
/// one, and the basin round a top is one cloud. The broad octave alone is
/// climbed: the fine one has a top every few pixels and would cut the sky
/// into confetti.
///
/// Where two basins meet is not where a cloud comes apart, whatever it looks
/// like on the broad octave: the shape that is drawn is the two octaves
/// mixed, and the fine one welds neighboring basins into one mass, so a seam
/// runs through the thick of a cloud as often as not. Nothing may be cut
/// along one - see what the caller does with this instead.
fn find_middles(broad: &[f32], w: i32, h: i32, rise: &mut Vec<f32>) {
    let n = (w * h) as usize;
    // Where each pixel steps next: its highest neighbor, or itself at a top.
    // Steps only ever go up, so there is no ring to walk round.
    let mut next: Vec<u32> = vec![0; n];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let mut best = i;
            let mut top = broad[i];
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let j = ((y + dy).rem_euclid(h) * w + (x + dx).rem_euclid(w)) as usize;
                    if broad[j] > top {
                        top = broad[j];
                        best = j;
                    }
                }
            }
            next[i] = best as u32;
        }
    }
    // Every chain resolved to its top, and every pixel on the way pointed
    // straight at it, so no step is walked twice.
    let mut top_of: Vec<u32> = vec![u32::MAX; n];
    let mut path: Vec<usize> = Vec::new();
    for start in 0..n {
        if top_of[start] != u32::MAX {
            continue;
        }
        path.clear();
        let mut i = start;
        while top_of[i] == u32::MAX && next[i] as usize != i {
            path.push(i);
            i = next[i] as usize;
        }
        let root = if top_of[i] != u32::MAX { top_of[i] } else { i as u32 };
        top_of[i] = root;
        for &p in &path {
            top_of[p] = root;
        }
    }
    let half = h / 2;
    rise.clear();
    rise.resize(n, 0.0);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let ty = top_of[i] as i32 / w;
            let r = ((ty - y + half).rem_euclid(h) - half) as f32;
            rise[i] = r.clamp(-SINK_REACH, SINK_REACH);
        }
    }
}

/// A box blur over the tile, wrapping both ways, run once across and once
/// down. The running sum is what keeps it cheap enough to sit inside a tile
/// that is rebuilt several times a second, and the window's two ends are
/// walked round by hand rather than taken modulo: at this size the divisions
/// a `rem_euclid` per pixel costs are most of the pass.
fn blur_wrapped(v: &mut [f32], tmp: &mut Vec<f32>, w: i32, h: i32, r: i32) {
    let span = (2 * r + 1) as f32;
    tmp.clear();
    tmp.resize(v.len(), 0.0);
    for y in 0..h {
        let row = (y * w) as usize;
        let mut sum: f32 = (-r..=r).map(|d| v[row + d.rem_euclid(w) as usize]).sum();
        // The pixel the window is about to leave behind, and the one it is
        // about to take in.
        let mut out = (-r).rem_euclid(w) as usize;
        let mut into = (r + 1).rem_euclid(w) as usize;
        for x in 0..w {
            tmp[row + x as usize] = sum / span;
            sum -= v[row + out];
            sum += v[row + into];
            out = if out + 1 == w as usize { 0 } else { out + 1 };
            into = if into + 1 == w as usize { 0 } else { into + 1 };
        }
    }
    let stride = w as usize;
    let last = ((h - 1) * w) as usize;
    for x in 0..w as usize {
        let mut sum: f32 = (-r..=r).map(|d| tmp[(d.rem_euclid(h) * w) as usize + x]).sum();
        let mut out = ((-r).rem_euclid(h) * w) as usize;
        let mut into = ((r + 1).rem_euclid(h) * w) as usize;
        let mut at = x;
        for _ in 0..h {
            v[at] = sum / span;
            sum -= tmp[out + x];
            sum += tmp[into + x];
            out = if out == last { 0 } else { out + stride };
            into = if into == last { 0 } else { into + stride };
            at += stride;
        }
    }
}

/// How far below the base a cloud's middle has to sink before the cloud is
/// gone. Over these rows its threshold climbs to one, which is past anything
/// the field reaches, so it thins from every edge at once and disappears. A
/// couple of rows would read as a cut; a hundred would leave haze hanging
/// under the weather for the whole height of the sky.
const SINK_ROWS: f64 = 22.0;

/// How far a cloud's middle is allowed to be read as being from one of its
/// own pixels. Rises are wrapped into half a tile either way, so two pixels
/// on opposite sides of a seam can come out a whole tile apart; nothing past
/// this distance changes a decision - the cloud is long gone or untouched -
/// so the value is pinned here before it is smoothed, and the wrap never
/// averages into a middle that is nowhere.
const SINK_REACH: f32 = 48.0;

/// How far the sink is smoothed sideways. It is a cloud's own number, so it
/// steps at the seam with the next cloud, and a seam found by climbing a
/// lattice field is a straight diagonal: left alone it cuts triangles of sky
/// out of the weather. Spread over a few pixels the step stops being a line
/// and the thinning falls back on the field's own contour.
const SINK_BLUR: i32 = 12;

/// The three tones a cloud pixel can take, mixed from the sky once per
/// rebuild.
struct Palette {
    core: u32,
    body: u32,
    under: u32,
}

/// One pixel of the tile against a threshold, or zero for sky. The threshold
/// is a parameter rather than a constant because the band round the base
/// shades the same field again with it raised, which is how a sinking cloud
/// thins along its own contour instead of being cut along the seam with its
/// neighbor.
fn shade(scratch: &[f32], x: i32, y: i32, seed: i32, cut: f64, p: &Palette) -> u32 {
    let d = scratch[(y * TILE_W + x) as usize] as f64 - cut;
    if d < 0.0 {
        return 0;
    }
    // A ragged pixel edge rather than a hard contour.
    if d < 0.045 && crate::util::hash2(x, y, seed ^ 0x2f1) > d / 0.045 {
        return 0;
    }
    // The bottom of a shape is in shade; the thick of it is brightest.
    let below = (y + 3).rem_euclid(TILE_H);
    let thins_below = (scratch[(below * TILE_W + x) as usize] as f64) < cut + 0.02;
    if thins_below {
        p.under
    } else if d > 0.16 {
        p.core
    } else {
        p.body
    }
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
    ensure_middles(layer, seed);

    // The palette leans on the sky it hangs in, so recoloring the sky
    // recolors the weather.
    let sky_top = hex_to_packed(&state.civ.world.sky_top);
    let sky_bottom = hex_to_packed(&state.civ.world.sky_bottom);
    let white = pack_rgba(236, 242, 248, 255);
    let pal = Palette {
        core: mix_packed(white, sky_top, 0.08),
        body: mix_packed(white, sky_top, 0.24),
        under: mix_packed(white, sky_bottom, 0.48),
    };

    // Lent out for the two shading passes, which read the field while they
    // write the tiles, and handed back at the end.
    let scratch = std::mem::take(&mut layer.scratch);
    let threshold = 0.86 - view.cloud_cover.clamp(0.0, 1.0) * 0.52;
    layer.px.clear();
    layer.px.resize(n, 0);
    for y in 0..TILE_H {
        for x in 0..TILE_W {
            layer.px[(y * TILE_W + x) as usize] =
                shade(&scratch, x, y, seed, threshold, &pal);
        }
    }

    // The band across the base. A pixel in row `e` of it is at world row
    // `base - h / 2 + e`, and `e + sink - h / 2` is how far the middle of the
    // cloud it belongs to has sunk below the base - the same number for every
    // pixel of one cloud, since the pixel's own row cancels out, which is what
    // lets a cloud thin as a whole. The band is exactly a tile tall because a
    // middle is never more than half a tile from its pixel, so above the band
    // every cloud is whole and below it none is.
    let half = TILE_H / 2;
    layer.edge.clear();
    layer.edge.resize(n, 0);
    for e in 0..TILE_H {
        let sy = (e + half) % TILE_H;
        let drop = (e - half) as f32;
        for x in 0..TILE_W {
            let i = (sy * TILE_W + x) as usize;
            // Raising a threshold only ever takes pixels away, so sky in the
            // whole tile is sky here too and nothing has to be shaded again
            // to find that out.
            let src = layer.px[i];
            if src == 0 {
                continue;
            }
            let sunk = (drop + layer.sink[i]) as f64;
            if sunk >= SINK_ROWS {
                continue;
            }
            // Above the base the threshold is the one the whole tile was
            // shaded against, which is the pixel already worked out.
            layer.edge[(e * TILE_W + x) as usize] = if sunk <= 0.0 {
                src
            } else {
                shade(&scratch, x, sy, seed, threshold + sunk / SINK_ROWS, &pal)
            };
        }
    }
    layer.scratch = scratch;
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
