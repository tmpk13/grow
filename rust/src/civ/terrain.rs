//! Procedural terrain for the settlement map.
//!
//! Two noise fields (elevation and moisture) decide what every cell is, and
//! resource deposits are scattered into the cells that suit them: stone and ore
//! in the high rock, clay along the water. Deposits hold a finite amount, so a
//! settlement that has emptied the ground near it has to reach further out.
//!
//! Everything here is a pure function of the seed and the parameters, so the
//! same seed always rebuilds the same map.

use serde::{Deserialize, Serialize};

use crate::rng::Rng;
use crate::util::{clamp01, clampi, hash2, lerp, smoothstep};
use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Cell {
    Grass = 0,
    Water = 1,
    Rock = 2,
    Sand = 3,
}

pub use crate::world::Zone;

impl Cell {
    pub fn from_u8(v: u8) -> Cell {
        match v {
            1 => Cell::Water,
            2 => Cell::Rock,
            3 => Cell::Sand,
            _ => Cell::Grass,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepositKind {
    Stone,
    Clay,
    Ore,
}

pub const DEPOSIT_KINDS: [DepositKind; 3] = [DepositKind::Stone, DepositKind::Clay, DepositKind::Ore];

impl DepositKind {
    pub fn id(self) -> &'static str {
        match self {
            DepositKind::Stone => "stone",
            DepositKind::Clay => "clay",
            DepositKind::Ore => "ore",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DepositConfig {
    pub density: f64,
    pub cluster_min: i32,
    pub cluster_max: i32,
    pub amount_min: i32,
    pub amount_max: i32,
}

impl Default for DepositConfig {
    fn default() -> Self {
        DepositConfig { density: 0.9, cluster_min: 2, cluster_max: 6, amount_min: 90, amount_max: 260 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DepositTable {
    pub stone: DepositConfig,
    pub clay: DepositConfig,
    pub ore: DepositConfig,
}

impl Default for DepositTable {
    fn default() -> Self {
        DepositTable {
            stone: DepositConfig { density: 0.9, cluster_min: 2, cluster_max: 6, amount_min: 90, amount_max: 260 },
            clay: DepositConfig { density: 0.7, cluster_min: 2, cluster_max: 5, amount_min: 70, amount_max: 200 },
            ore: DepositConfig { density: 0.35, cluster_min: 1, cluster_max: 4, amount_min: 60, amount_max: 180 },
        }
    }
}

impl DepositTable {
    pub fn get(&self, kind: DepositKind) -> DepositConfig {
        match kind {
            DepositKind::Stone => self.stone,
            DepositKind::Clay => self.clay,
            DepositKind::Ore => self.ore,
        }
    }

    pub fn get_mut(&mut self, kind: DepositKind) -> &mut DepositConfig {
        match kind {
            DepositKind::Stone => &mut self.stone,
            DepositKind::Clay => &mut self.clay,
            DepositKind::Ore => &mut self.ore,
        }
    }
}

/// Rivers are carved after the noise, not sampled out of it: a channel that
/// runs downhill from a spring to the sea is a path, and a path is what boats
/// and bridges need. Everything here is in cells.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RiverConfig {
    /// Springs attempted per 10000 cells, so a bigger map gets more rivers
    /// rather than the same few stretched across it.
    pub density: f64,
    /// Half width of the channel at the mouth, in cells. The head of the river
    /// is always one cell wide.
    pub width: f64,
    /// Shortest run worth keeping; a trickle that dies in twenty cells is
    /// carved and then thrown away.
    pub min_length: i32,
    pub max_length: i32,
    /// How far the course is pushed sideways off the steepest descent.
    pub meander: f64,
    /// How much the damp ground along a bank feeds a farm.
    pub bank_fertility: f64,
}

impl Default for RiverConfig {
    fn default() -> Self {
        RiverConfig {
            density: 6.0,
            width: 1.6,
            min_length: 18,
            max_length: 600,
            meander: 0.55,
            bank_fertility: 0.55,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TerrainConfig {
    pub scale: f64,
    pub octaves: i32,
    pub persistence: f64,
    pub warp: f64,
    pub water_level: f64,
    pub sand_band: f64,
    pub rock_level: f64,
    pub moist_scale: f64,
    pub fertility: f64,
    /// Wild growth simulated before the settlers arrive, in simulated seconds.
    pub warmup: f64,
    /// How lush the map is: scales seeding rate and how many plants of each
    /// species the land carries. Wild food and timber both follow this.
    pub wildness: f64,
    pub deposits: DepositTable,
    pub rivers: RiverConfig,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        TerrainConfig {
            scale: 14.0,
            octaves: 4,
            persistence: 0.5,
            warp: 0.35,
            water_level: 0.32,
            sand_band: 0.04,
            rock_level: 0.68,
            moist_scale: 22.0,
            fertility: 0.6,
            warmup: 420.0,
            wildness: 2.2,
            deposits: DepositTable::default(),
            rivers: RiverConfig::default(),
        }
    }
}

/// Value noise with bilinear interpolation, summed over octaves. `hash2` is the
/// same stable hash the plant shading uses, so terrain is reproducible without
/// carrying an RNG stream through the sampling.
fn value_noise(x: f64, y: f64, seed: i32) -> f64 {
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = smoothstep(x - x0);
    let fy = smoothstep(y - y0);
    let xi = x0 as i32;
    let yi = y0 as i32;
    let a = hash2(xi, yi, seed);
    let b = hash2(xi + 1, yi, seed);
    let c = hash2(xi, yi + 1, seed);
    let d = hash2(xi + 1, yi + 1, seed);
    lerp(lerp(a, b, fx), lerp(c, d, fx), fy)
}

pub fn fbm(x: f64, y: f64, seed: i32, octaves: i32, persistence: f64) -> f64 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut freq = 1.0;
    for o in 0..octaves {
        sum += value_noise(x * freq, y * freq, seed.wrapping_add(o * 7919)) * amp;
        norm += amp;
        amp *= persistence;
        freq *= 2.0;
    }
    if norm > 0.0 {
        sum / norm
    } else {
        0.0
    }
}

#[derive(Clone, Debug)]
pub struct Deposit {
    pub id: i32,
    pub kind: DepositKind,
    pub col: i32,
    pub row: i32,
    pub amount: f64,
    pub max: f64,
    pub seed: u32,
}

pub struct DepositCount {
    pub cells: usize,
    pub amount: f64,
}

/// One carved watercourse, kept as the polyline it was cut along so a dock or
/// a ford can be placed against a named river rather than against "some water".
#[derive(Clone, Debug)]
pub struct River {
    pub id: i32,
    pub name: String,
    pub path: Vec<(i32, i32)>,
    /// True when the course reached standing water or the map edge instead of
    /// petering out in a hollow.
    pub reaches_sea: bool,
}

pub struct Terrain {
    pub cols: i32,
    pub rows: i32,
    pub seed: u32,
    /// The corner of the map the generator must leave alone, in cells: a map
    /// that has been made larger under a running settlement is already built
    /// on, and a river cut through it or a deposit dropped into it would be the
    /// ground moving under a town. Zero while a map is being made from nothing,
    /// which is every case but that one.
    frozen: (i32, i32),
    pub elev: Vec<f32>,
    pub moist: Vec<f32>,
    pub fert: Vec<f32>,
    pub kind: Vec<u8>,
    pub deposit_index: Vec<i32>,
    pub deposits: Vec<Deposit>,
    pub water_cells: usize,
    /// Which river a water cell belongs to, or 0 for lake and sea. Rivers are
    /// drawn with a current and are the only water a small boat will follow
    /// inland.
    pub river_index: Vec<i32>,
    /// Direction the water runs, as an index into `FLOW_DIRS`, for the ripples
    /// and for pushing a boat along.
    pub flow: Vec<i8>,
    pub rivers: Vec<River>,
    /// What the wilderness may do with each cell. Authored rather than
    /// generated, so it survives a reload the way the deposits do while the
    /// rest of the map is made again from the seed.
    pub zone: Vec<u8>,
}

/// The eight steps a course may take, in the order a direction index means.
pub const FLOW_DIRS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

impl Terrain {
    pub fn new(world: &World, cfg: &TerrainConfig, seed: u32) -> Self {
        let mut t = Terrain {
            cols: world.cols,
            rows: world.rows,
            seed,
            frozen: (0, 0),
            elev: Vec::new(),
            moist: Vec::new(),
            fert: Vec::new(),
            kind: Vec::new(),
            deposit_index: Vec::new(),
            deposits: Vec::new(),
            water_cells: 0,
            river_index: Vec::new(),
            flow: Vec::new(),
            rivers: Vec::new(),
            zone: Vec::new(),
        };
        t.generate(cfg);
        t
    }

    fn generate(&mut self, cfg: &TerrainConfig) {
        let n = (self.cols * self.rows) as usize;
        self.elev = vec![0.0; n];
        self.moist = vec![0.0; n];
        self.fert = vec![0.0; n];
        self.kind = vec![0; n];
        self.deposit_index = vec![0; n];
        self.river_index = vec![0; n];
        self.flow = vec![-1; n];
        // Zones are authored rather than generated, so they are not wiped
        // here; a map made from nothing simply has none.
        self.zone.resize(n, 0);
        self.deposits.clear();
        self.rivers.clear();

        self.fill_noise(cfg);
        self.carve_rivers(cfg);
        self.scatter_deposits(cfg);
        self.water_cells = self.kind.iter().filter(|&&k| k == Cell::Water as u8).count();
    }

    /// A larger map with the old one still standing in the corner of it.
    ///
    /// Everything the noise decides is a function of the cell's own position,
    /// so the ground that was already there comes out of the generator the
    /// same and is simply copied across into the wider stride. Rivers and
    /// deposits are not: they are placed by a walk, so the new land gets its
    /// own and the old land keeps what it had. A course traced into the old
    /// map stops at the boundary rather than cutting a channel through
    /// somebody's town.
    pub fn expand(&mut self, world: &World, cfg: &TerrainConfig) {
        let (old_cols, old_rows) = (self.cols, self.rows);
        let (cols, rows) = (world.cols.max(old_cols), world.rows.max(old_rows));
        if cols == old_cols && rows == old_rows {
            return;
        }
        let (elev, moist, fert) = (
            std::mem::take(&mut self.elev),
            std::mem::take(&mut self.moist),
            std::mem::take(&mut self.fert),
        );
        let (kind, dindex, rindex, flow) = (
            std::mem::take(&mut self.kind),
            std::mem::take(&mut self.deposit_index),
            std::mem::take(&mut self.river_index),
            std::mem::take(&mut self.flow),
        );
        let zone = std::mem::take(&mut self.zone);
        self.cols = cols;
        self.rows = rows;
        let n = (cols * rows) as usize;
        self.elev = vec![0.0; n];
        self.moist = vec![0.0; n];
        self.fert = vec![0.0; n];
        self.kind = vec![0; n];
        self.deposit_index = vec![0; n];
        self.river_index = vec![0; n];
        self.flow = vec![-1; n];
        self.zone = vec![0; n];

        self.fill_noise(cfg);
        for r in 0..old_rows {
            for c in 0..old_cols {
                let from = (r * old_cols + c) as usize;
                let to = (r * cols + c) as usize;
                self.elev[to] = elev[from];
                self.moist[to] = moist[from];
                self.fert[to] = fert[from];
                self.kind[to] = kind[from];
                self.deposit_index[to] = dindex[from];
                self.river_index[to] = rindex[from];
                self.flow[to] = flow[from];
                self.zone[to] = zone.get(from).copied().unwrap_or(0);
            }
        }

        self.frozen = (old_cols, old_rows);
        self.carve_rivers(cfg);
        self.scatter_deposits(cfg);
        self.frozen = (0, 0);
        self.water_cells = self.kind.iter().filter(|&&k| k == Cell::Water as u8).count();
    }

    /// Ground that has already been walked on. Nothing the generator places by
    /// hand may touch it.
    fn frozen_at(&self, c: i32, r: i32) -> bool {
        c < self.frozen.0 && r < self.frozen.1
    }

    /// Elevation, moisture and what that makes the cell, for every cell of the
    /// map. A pure function of the seed and the position, which is what lets a
    /// map be made larger without the ground under a town changing.
    fn fill_noise(&mut self, cfg: &TerrainConfig) {
        let (cols, rows) = (self.cols, self.rows);
        let scale = cfg.scale.max(2.0);
        let mscale = cfg.moist_scale.max(2.0);
        let oct = clampi(cfg.octaves, 1, 6);
        let pers = cfg.persistence.clamp(0.1, 0.9);
        let seed = self.seed as i32;

        for r in 0..rows {
            for c in 0..cols {
                let i = (r * cols + c) as usize;
                // Domain warp so coastlines meander instead of following the
                // noise grid; without it lakes come out suspiciously round.
                let wx = fbm(c as f64 / (scale * 2.0), r as f64 / (scale * 2.0), seed.wrapping_add(101), 2, 0.5) - 0.5;
                let wy = fbm(c as f64 / (scale * 2.0), r as f64 / (scale * 2.0), seed.wrapping_add(202), 2, 0.5) - 0.5;
                let x = c as f64 / scale + wx * cfg.warp * 2.0;
                let y = r as f64 / scale + wy * cfg.warp * 2.0;
                let e = fbm(x, y, seed, oct, pers);
                let m = fbm(
                    c as f64 / mscale + 31.7,
                    r as f64 / mscale + 12.3,
                    seed.wrapping_add(5501),
                    3,
                    0.55,
                );
                self.elev[i] = e as f32;
                self.moist[i] = m as f32;
                self.kind[i] = if e < cfg.water_level {
                    Cell::Water as u8
                } else if e < cfg.water_level + cfg.sand_band {
                    Cell::Sand as u8
                } else if e > cfg.rock_level {
                    Cell::Rock as u8
                } else {
                    Cell::Grass as u8
                };
                let wetness = clamp01(m * 0.7 + (1.0 - (e - cfg.water_level - 0.12).abs() * 2.2) * 0.5);
                self.fert[i] = if self.kind[i] == Cell::Grass as u8 {
                    clamp01(wetness * cfg.fertility * 1.6) as f32
                } else {
                    0.0
                };
            }
        }
    }

    // ---- rivers ----------------------------------------------------------

    /// Springs high up, then downhill to the sea. Each course is traced first
    /// and only cut once it has proved itself long enough, so the map does not
    /// end up pitted with puddles where a trickle gave up.
    fn carve_rivers(&mut self, cfg: &TerrainConfig) {
        let rc = cfg.rivers;
        if rc.density <= 0.0 || rc.width <= 0.0 {
            return;
        }
        let area = (self.cols * self.rows) as f64;
        let springs = ((rc.density * area / 10000.0).round() as i32).clamp(0, 64);
        if springs == 0 {
            return;
        }
        let mut rng = Rng::new(self.seed ^ 0x1f35_d0c7);
        for _ in 0..springs {
            let source = match self.pick_spring(cfg, &mut rng) {
                Some(s) => s,
                None => continue,
            };
            let (path, reached) = self.trace_course(source, &rc, &mut rng);
            if (path.len() as i32) < rc.min_length {
                continue;
            }
            let id = self.rivers.len() as i32 + 1;
            self.cut_channel(&path, &rc, cfg, id);
            let name = crate::civ::names::river_name(&mut rng);
            self.rivers.push(River { id, name, path, reaches_sea: reached });
        }
    }

    /// A spring wants high ground that is not already wet and not already the
    /// head of another river, so courses do not stack on one ridge.
    fn pick_spring(&self, cfg: &TerrainConfig, rng: &mut Rng) -> Option<(i32, i32)> {
        let high = (cfg.water_level + (cfg.rock_level - cfg.water_level) * 0.55).max(0.0);
        let mut best: Option<(i32, i32)> = None;
        let mut best_e = f64::NEG_INFINITY;
        for _ in 0..90 {
            let c = rng.int(1, (self.cols - 2).max(1));
            let r = rng.int(1, (self.rows - 2).max(1));
            let i = self.idx(c, r);
            if self.kind[i] == Cell::Water as u8 || self.river_index[i] != 0 {
                continue;
            }
            if self.frozen_at(c, r) {
                continue;
            }
            let e = self.elev[i] as f64;
            if e < high {
                continue;
            }
            // Keep springs apart: a source inside another river's headwaters
            // just re-cuts the same valley.
            if self.near_river(c, r, 6) {
                continue;
            }
            if e > best_e {
                best_e = e;
                best = Some((c, r));
            }
        }
        best
    }

    fn near_river(&self, c: i32, r: i32, radius: i32) -> bool {
        for y in r - radius..=r + radius {
            for x in c - radius..=c + radius {
                if self.in_bounds(x, y) && self.river_index[self.idx(x, y)] != 0 {
                    return true;
                }
            }
        }
        false
    }

    /// Steepest descent with a memory. The remembered direction is what keeps a
    /// course from zig-zagging cell to cell across a flat, and the sideways
    /// push is what bends it into a meander instead of a straight fall line.
    fn trace_course(
        &self,
        source: (i32, i32),
        rc: &RiverConfig,
        rng: &mut Rng,
    ) -> (Vec<(i32, i32)>, bool) {
        let mut path: Vec<(i32, i32)> = Vec::new();
        let mut seen: Vec<u8> = vec![0; (self.cols * self.rows) as usize];
        let (mut c, mut r) = source;
        let mut heading = rng.int(0, 7) as usize;
        let phase = rng.range(0.0, 100.0);
        let max = rc.max_length.max(rc.min_length);
        for step in 0..max {
            // A course that reaches the old map ends there. It is not a river
            // that stops: it is a river running into ground that already has
            // its own water, and cutting on through it would put a channel
            // where somebody built a house.
            if self.frozen_at(c, r) {
                break;
            }
            let i = self.idx(c, r);
            if seen[i] != 0 {
                break;
            }
            seen[i] = 1;
            path.push((c, r));
            if self.kind[i] == Cell::Water as u8 && step > 0 {
                return (path, true);
            }
            // A meander is a slow sideways wobble along the course, so the bend
            // is a function of how far downstream we are rather than of noise
            // per cell.
            let wobble = ((step as f64 * 0.11 + phase).sin() * rc.meander * 2.4).round() as i32;
            let mut best = None;
            let mut best_score = f64::INFINITY;
            for (d, (dx, dy)) in FLOW_DIRS.iter().enumerate() {
                let nc = c + dx;
                let nr = r + dy;
                if !self.in_bounds(nc, nr) {
                    // Running off the edge is a mouth like any other.
                    return (path, true);
                }
                let ni = self.idx(nc, nr);
                if seen[ni] != 0 {
                    continue;
                }
                let turn = ((d as i32 - heading as i32 + 12) % 8 - 4).abs() as f64;
                if turn > 2.0 {
                    continue;
                }
                let side = ((d as i32 - heading as i32 + 12) % 8 - 4) as f64;
                let mut score = self.elev[ni] as f64;
                score += turn * 0.006;
                score -= side * wobble as f64 * 0.004;
                if self.kind[ni] == Cell::Water as u8 {
                    score -= 0.5;
                }
                if score < best_score {
                    best_score = score;
                    best = Some((d, nc, nr));
                }
            }
            let (d, nc, nr) = match best {
                Some(v) => v,
                None => break,
            };
            heading = d;
            c = nc;
            r = nr;
        }
        (path, false)
    }

    /// Cuts the traced course into the map: the channel widens downstream, the
    /// bed drops below the water line, and the ground either side is left damp.
    fn cut_channel(&mut self, path: &[(i32, i32)], rc: &RiverConfig, cfg: &TerrainConfig, id: i32) {
        let n = path.len().max(1) as f64;
        let bed = (cfg.water_level - 0.05).max(0.0) as f32;
        for (step, &(c, r)) in path.iter().enumerate() {
            let t = step as f64 / n;
            let half = (rc.width * (0.35 + t * 0.85)).max(0.0);
            let dir = if step + 1 < path.len() {
                let (nc, nr) = path[step + 1];
                FLOW_DIRS
                    .iter()
                    .position(|&(dx, dy)| dx == nc - c && dy == nr - r)
                    .unwrap_or(0) as i8
            } else {
                -1
            };
            let span = half.ceil() as i32;
            for y in r - span - 1..=r + span + 1 {
                for x in c - span - 1..=c + span + 1 {
                    if !self.in_bounds(x, y) {
                        continue;
                    }
                    if self.frozen_at(x, y) {
                        continue;
                    }
                    let d = ((x - c) as f64).hypot((y - r) as f64);
                    let i = self.idx(x, y);
                    if d <= half.max(0.5) {
                        self.kind[i] = Cell::Water as u8;
                        self.elev[i] = self.elev[i].min(bed);
                        self.fert[i] = 0.0;
                        if self.river_index[i] == 0 {
                            self.river_index[i] = id;
                        }
                        if dir >= 0 {
                            self.flow[i] = dir;
                        }
                        self.deposit_index[i] = 0;
                    } else if d <= half + 1.6 && self.kind[i] != Cell::Water as u8 {
                        // Banks: sand right at the edge, damp ground behind it.
                        if self.kind[i] == Cell::Grass as u8 {
                            self.fert[i] = clamp01(
                                self.fert[i] as f64 + rc.bank_fertility * (1.0 - (d - half) / 1.6),
                            ) as f32;
                        } else if self.kind[i] == Cell::Rock as u8 && d <= half + 0.9 {
                            self.kind[i] = Cell::Sand as u8;
                        }
                    }
                }
            }
        }
    }

    pub fn is_river(&self, c: i32, r: i32) -> bool {
        self.in_bounds(c, r) && self.river_index[self.idx(c, r)] > 0
    }

    /// Cells the deposits and the site planner treat as navigable: any water,
    /// river or open, so a dock built on a lake still works.
    pub fn navigable(&self, c: i32, r: i32) -> bool {
        self.in_bounds(c, r) && self.kind[self.idx(c, r)] == Cell::Water as u8
    }

    fn scatter_deposits(&mut self, cfg: &TerrainConfig) {
        let mut rng = Rng::new(self.seed ^ 0x5bf0_3635);
        let area = (self.cols * self.rows) as f64;
        for kind in DEPOSIT_KINDS {
            let dc = cfg.deposits.get(kind);
            let clusters = (dc.density * area / 100.0).round() as i32;
            for _ in 0..clusters {
                let seed_cell = self.pick_seed_cell(kind, cfg, &mut rng);
                if seed_cell < 0 {
                    continue;
                }
                let size = rng.int(dc.cluster_min, dc.cluster_max);
                self.grow_cluster(seed_cell, size, kind, &dc, &mut rng);
            }
        }
    }

    /// Deposits sit where their story puts them: stone and ore in high ground,
    /// clay in the damp low ground next to water.
    fn pick_seed_cell(&self, kind: DepositKind, cfg: &TerrainConfig, rng: &mut Rng) -> i32 {
        for _ in 0..60 {
            let c = rng.int(0, self.cols - 1);
            let r = rng.int(0, self.rows - 1);
            let i = (r * self.cols + c) as usize;
            if self.kind[i] == Cell::Water as u8 || self.deposit_index[i] != 0 {
                continue;
            }
            if self.frozen_at(c, r) {
                continue;
            }
            let e = self.elev[i] as f64;
            match kind {
                DepositKind::Stone => {
                    if self.kind[i] == Cell::Rock as u8 || e > cfg.rock_level - 0.12 {
                        return i as i32;
                    }
                }
                DepositKind::Ore => {
                    if self.kind[i] == Cell::Rock as u8 && rng.chance(0.7) {
                        return i as i32;
                    }
                }
                DepositKind::Clay => {
                    if self.near_water(c, r, 3) && self.kind[i] != Cell::Rock as u8 {
                        return i as i32;
                    }
                }
            }
        }
        -1
    }

    fn grow_cluster(&mut self, seed_cell: i32, size: i32, kind: DepositKind, dc: &DepositConfig, rng: &mut Rng) {
        let mut c = seed_cell % self.cols;
        let mut r = seed_cell / self.cols;
        for _ in 0..size {
            let i = (r * self.cols + c) as usize;
            if c >= 0
                && c < self.cols
                && r >= 0
                && r < self.rows
                && self.kind[i] != Cell::Water as u8
                && self.deposit_index[i] == 0
                && !self.frozen_at(c, r)
            {
                let amount = rng.int(dc.amount_min, dc.amount_max) as f64;
                self.deposits.push(Deposit {
                    id: self.deposits.len() as i32 + 1,
                    kind,
                    col: c,
                    row: r,
                    amount,
                    max: amount,
                    seed: rng.seed(),
                });
                self.deposit_index[i] = self.deposits.len() as i32;
            }
            c += rng.int(-1, 1);
            r += rng.int(-1, 1);
            c = clampi(c, 0, self.cols - 1);
            r = clampi(r, 0, self.rows - 1);
        }
    }

    pub fn idx(&self, c: i32, r: i32) -> usize {
        (r * self.cols + c) as usize
    }

    pub fn in_bounds(&self, c: i32, r: i32) -> bool {
        c >= 0 && c < self.cols && r >= 0 && r < self.rows
    }

    pub fn type_at(&self, c: i32, r: i32) -> Cell {
        if self.in_bounds(c, r) {
            Cell::from_u8(self.kind[self.idx(c, r)])
        } else {
            Cell::Water
        }
    }

    pub fn is_water(&self, c: i32, r: i32) -> bool {
        self.type_at(c, r) == Cell::Water
    }

    /// What the wilderness may do with a cell.
    pub fn zone_at(&self, c: i32, r: i32) -> Zone {
        if !self.in_bounds(c, r) {
            return Zone::Any;
        }
        match self.zone.get(self.idx(c, r)) {
            Some(&v) => Zone::from_u8(v),
            None => Zone::Any,
        }
    }

    /// Draws a zone on one cell. Nothing else about the map changes: a zone is
    /// about what takes root there, not about what the ground is.
    pub fn set_zone(&mut self, c: i32, r: i32, zone: Zone) {
        if !self.in_bounds(c, r) {
            return;
        }
        let n = (self.cols * self.rows) as usize;
        if self.zone.len() < n {
            self.zone.resize(n, 0);
        }
        let i = self.idx(c, r);
        self.zone[i] = zone as u8;
    }

    /// Whether anything has been zoned at all, which is what keeps the whole
    /// grid out of the spawn loop on a map nobody has drawn on.
    pub fn any_zones(&self) -> bool {
        self.zone.iter().any(|&z| z != 0)
    }

    pub fn is_buildable(&self, c: i32, r: i32) -> bool {
        matches!(self.type_at(c, r), Cell::Grass | Cell::Sand | Cell::Rock)
    }

    pub fn fertility(&self, c: i32, r: i32) -> f64 {
        if self.in_bounds(c, r) {
            self.fert[self.idx(c, r)] as f64
        } else {
            0.0
        }
    }

    pub fn near_water(&self, c: i32, r: i32, radius: i32) -> bool {
        for y in r - radius..=r + radius {
            for x in c - radius..=c + radius {
                if self.in_bounds(x, y) && self.kind[self.idx(x, y)] == Cell::Water as u8 {
                    return true;
                }
            }
        }
        false
    }

    pub fn deposit_at(&self, c: i32, r: i32) -> Option<usize> {
        if !self.in_bounds(c, r) {
            return None;
        }
        let di = self.deposit_index[self.idx(c, r)];
        if di > 0 {
            Some(di as usize - 1)
        } else {
            None
        }
    }

    /// Nearest deposit of a kind with anything left in it.
    pub fn find_deposit(&self, kind: DepositKind, col: i32, row: i32, radius: f64) -> Option<usize> {
        let mut best = None;
        let mut best_d = f64::INFINITY;
        for (i, d) in self.deposits.iter().enumerate() {
            if d.kind != kind || d.amount <= 0.0 {
                continue;
            }
            let dx = (d.col - col) as f64;
            let dy = (d.row - row) as f64;
            let dist = dx * dx + dy * dy;
            if dist > radius * radius || dist >= best_d {
                continue;
            }
            best = Some(i);
            best_d = dist;
        }
        best
    }

    pub fn deposit_by_id(&self, id: i32) -> Option<usize> {
        self.deposits.iter().position(|d| d.id == id)
    }

    pub fn count_deposits(&self, kind: DepositKind) -> DepositCount {
        let mut cells = 0;
        let mut amount = 0.0;
        for d in &self.deposits {
            if d.kind != kind {
                continue;
            }
            if d.amount > 0.0 {
                cells += 1;
            }
            amount += d.amount;
        }
        DepositCount { cells, amount }
    }

    pub fn take(&mut self, index: usize, n: f64) -> f64 {
        let (col, row, got) = {
            let d = &mut self.deposits[index];
            let got = d.amount.min(n);
            d.amount -= got;
            (d.col, d.row, got)
        };
        if self.deposits[index].amount <= 0.0 {
            let i = self.idx(col, row);
            self.deposit_index[i] = 0;
        }
        got
    }

    /// A tolerable spot for the first storehouse: flat, buildable, near fertile
    /// ground and not on top of a deposit.
    pub fn find_start_cell(&self, rng: &mut Rng) -> (i32, i32) {
        let mut best: Option<(i32, i32)> = None;
        let mut best_score = f64::NEG_INFINITY;
        for _ in 0..400 {
            let c = rng.int(2, self.cols - 3);
            let r = rng.int(2, self.rows - 3);
            if !self.is_buildable(c, r) || self.deposit_at(c, r).is_some() {
                continue;
            }
            let mut score = self.fertility(c, r) * 2.0;
            let mut openness = 0.0;
            for y in r - 2..=r + 2 {
                for x in c - 2..=c + 2 {
                    if self.is_buildable(x, y) && self.deposit_at(x, y).is_none() {
                        openness += 1.0;
                    }
                }
            }
            score += openness / 25.0;
            if self.near_water(c, r, 6) {
                score += 0.5;
            }
            // Middle of the map reads better than a corner.
            let dx = (c as f64 - self.cols as f64 / 2.0) / self.cols as f64;
            let dy = (r as f64 - self.rows as f64 / 2.0) / self.rows as f64;
            score -= (dx * dx + dy * dy).sqrt();
            if score > best_score {
                best_score = score;
                best = Some((c, r));
            }
        }
        best.unwrap_or((self.cols / 2, self.rows / 2))
    }
}
