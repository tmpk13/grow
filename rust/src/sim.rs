//! Simulation: spawning, growth scheduling, grid bookkeeping and compositing
//! the world pixel buffer.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::plant::{Mat, Plant, RasterEnv, Ramps, Scratch};
use crate::rng::Rng;
use crate::sampler::{Bands, Materials};
use crate::species::{effective_limits, SizeClass, Species};
use crate::state::State;
use crate::util::{clamp, clampi, hash2, hex_to_packed, mix_packed, pack_rgba};
use crate::world::{World, WorldConfig};

const SHADOW_COLOR: u32 = pack_rgba(6, 10, 14, 255);

/// How far over a cut plant goes before it is taken off the map. Not the whole
/// quarter turn: a trunk on the ground still has its own thickness, and the
/// last few degrees are the ones that read as sinking into the soil.
const FALL_ANGLE: f64 = 1.42;

/// Ramps are looked up per pixel during shading, so they are resolved once per
/// species and cached until the sampling boxes change.
#[derive(Default)]
pub struct Env {
    version: u32,
    cache: HashMap<String, Ramps>,
}

fn empty_ramps() -> Ramps {
    std::array::from_fn(|_| Rc::new(Bands::fallback(Vec::new())))
}

impl Env {
    pub fn invalidate(&mut self) {
        self.version = u32::MAX;
        self.cache.clear();
    }

    pub fn ramps_for(&mut self, materials: &Materials, species: &Species) -> Ramps {
        if self.version != materials.version {
            self.version = materials.version;
            self.cache.clear();
        }
        if let Some(hit) = self.cache.get(&species.id) {
            return hit.clone();
        }
        let mut ramps = empty_ramps();
        for mat in Mat::all() {
            ramps[mat as usize] = materials.bands(species.slot(mat));
        }
        self.cache.insert(species.id.clone(), ramps.clone());
        ramps
    }
}

pub struct Stats {
    pub total: usize,
    pub per_species: HashMap<String, usize>,
    pub time: f64,
    pub ticks: u64,
}

/// What foliage does where it covers a settler. Somebody walking behind a bush
/// is behind it, which is right and also makes them hard to follow; the other
/// two let the bush stay a bush and the settler stay findable.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Foliage {
    /// A leaf is a leaf. What a plant looks like from in front.
    Solid,
    /// Every other pixel of the covering foliage is left out, so the settler
    /// shows through it in a screen pattern rather than as a ghost.
    Hatched,
    /// The covering foliage is mixed over the settler at this much.
    Faded(f64),
}

pub struct Sim {
    /// The world config is held separately from the state so a second sim (the
    /// settlement map) can run the same species on a grid of its own size.
    pub world_cfg: WorldConfig,
    pub world: World,
    pub env: Env,
    pub scratch: Scratch,
    pub rng: Rng,
    pub plants: Vec<Plant>,
    pub next_id: i32,
    pub time: f64,
    pub ticks: u64,
    pub buffer: Vec<u32>,
    pub buffer_dirty: bool,
    pub raster_queue: VecDeque<i32>,
    /// How lush this world is: scales both how often a species seeds and how
    /// many instances of it the world carries. The settlement map runs richer
    /// than the lab because it is larger and because people eat what grows.
    pub wild_scale: f64,
    /// Seconds a cut plant takes to go over. Set from the settlement, which is
    /// the only place anything is ever cut down.
    pub fall_time: f64,
}

impl Sim {
    pub fn new(state: &State, world_cfg: WorldConfig) -> Self {
        let world = World::new(&world_cfg);
        let mut sim = Sim {
            world_cfg,
            world,
            env: Env::default(),
            scratch: Scratch::default(),
            rng: Rng::new(state.seed),
            plants: Vec::new(),
            next_id: 1,
            time: 0.0,
            ticks: 0,
            buffer: Vec::new(),
            buffer_dirty: true,
            raster_queue: VecDeque::new(),
            wild_scale: 1.0,
            fall_time: 1.2,
        };
        sim.reset(state.seed);
        sim
    }

    pub fn reset(&mut self, seed: u32) {
        self.world.configure(&self.world_cfg);
        self.world.clear();
        self.rng = Rng::new(seed);
        self.plants.clear();
        self.next_id = 1;
        self.time = 0.0;
        self.ticks = 0;
        self.buffer = vec![0; (self.world.px_w * self.world.px_h) as usize];
        self.buffer_dirty = true;
        self.raster_queue.clear();
        self.env.invalidate();
    }

    pub fn species_of<'a>(&self, state: &'a State, plant: &Plant) -> Option<&'a Species> {
        state.species.iter().find(|s| s.id == plant.species_id)
    }

    pub fn step(&mut self, state: &State, dt: f64, blocked: Option<&[u8]>) {
        self.time += dt;
        self.ticks += 1;
        self.spawn_phase(state, dt, blocked);
        self.grow_all(state, dt, blocked, None);
    }

    /// Grows a wilderness onto part of a world without letting the rest of it
    /// move.
    ///
    /// This is what a map made larger under a running settlement needs: the new
    /// land has to arrive with something growing on it, and the old land is not
    /// supposed to jump forward a week while that happens. `held` marks the
    /// cells to leave alone, which is the same mask that stops anything seeding
    /// there. The clock does not advance: no time passes in the world, this is
    /// ground that was always there being caught up with.
    pub fn warm_region(&mut self, state: &State, seconds: f64, dt: f64, held: &[u8]) {
        let step = dt.max(0.001);
        let mut t = 0.0;
        while t < seconds {
            self.spawn_phase(state, step, Some(held));
            self.grow_all(state, step, Some(held), Some(held));
            t += step;
        }
    }

    /// One step of growth for every plant, skipping any rooted in a held cell.
    fn grow_all(
        &mut self,
        state: &State,
        dt: f64,
        blocked: Option<&[u8]>,
        held: Option<&[u8]>,
    ) {
        let fall = self.fall_time.max(0.05);
        let cols = self.world.cols;
        for i in (0..self.plants.len()).rev() {
            if let Some(held) = held {
                let (col, row) = (self.plants[i].col, self.plants[i].row);
                if held.get((row * cols + col) as usize).is_some_and(|&h| h != 0) {
                    continue;
                }
            }
            if self.plants[i].felled > 0.0 {
                // Coming down. Nothing grows, shades or seeds from here: the
                // only thing left to do with it is finish the fall. The buffer
                // is not marked dirty for this, because the settlement redraws
                // what the camera can see every frame anyway and marking it
                // would rebuild the cached ground once per tick of the fall.
                self.plants[i].felled += dt / fall;
                if self.plants[i].felled >= 1.0 {
                    self.remove_plant_at(i);
                }
                continue;
            }
            let species = match state.species.iter().find(|s| s.id == self.plants[i].species_id) {
                Some(s) => s,
                None => {
                    // The species was deleted from the project; so is anything
                    // still growing from it.
                    self.remove_plant_at(i);
                    continue;
                }
            };
            self.plants[i].grow(dt, species, &mut self.world, blocked);
            if !self.plants[i].alive {
                self.remove_plant_at(i);
            } else if self.plants[i].dirty {
                let id = self.plants[i].id;
                if !self.raster_queue.contains(&id) {
                    self.raster_queue.push_back(id);
                }
            }
        }
    }

    fn spawn_phase(&mut self, state: &State, dt: f64, blocked: Option<&[u8]>) {
        for (si, sp) in state.species.iter().enumerate() {
            if !sp.enabled {
                continue;
            }
            let limits = effective_limits(sp, &state.class_limits);
            let scale = if self.wild_scale > 0.0 { self.wild_scale } else { 1.0 };
            let mine: Vec<(i32, i32)> = self
                .plants
                .iter()
                .filter(|p| p.species_id == sp.id && p.standing())
                .map(|p| (p.col, p.row))
                .collect();
            if mine.len() as f64 >= limits.max_instances as f64 * scale {
                continue;
            }

            let mut attempts = sp.spawn.rate * scale * dt;
            while attempts > 0.0 {
                if attempts >= 1.0 || self.rng.chance(attempts) {
                    let col = self.rng.int(0, self.world.cols - 1);
                    let row = self.rng.int(0, self.world.rows - 1);
                    self.try_spawn(state, si, col, row, blocked);
                }
                attempts -= 1.0;
            }

            // Offspring land somewhere on the ring around the parent, anywhere
            // in the area rather than only left or right of it.
            for (pcol, prow) in mine {
                if !self.rng.chance(sp.spread.rate * scale * dt) {
                    continue;
                }
                let dist = self.rng.range(sp.spread.radius_min, sp.spread.radius_max);
                let a = self.rng.range(0.0, std::f64::consts::PI * 2.0);
                let col = (pcol as f64 + a.cos() * dist).round() as i32;
                let row = (prow as f64 + a.sin() * dist).round() as i32;
                self.try_spawn(state, si, col, row, blocked);
            }
        }
    }

    pub fn try_spawn(
        &mut self,
        state: &State,
        species_index: usize,
        col: i32,
        row: i32,
        blocked: Option<&[u8]>,
    ) -> Option<usize> {
        let sp = &state.species[species_index];
        let c = clampi(col, 0, self.world.cols - 1);
        let r = clampi(row, 0, self.world.rows - 1);
        if let Some(blocked) = blocked {
            if blocked[(r * self.world.cols + c) as usize] != 0 {
                return None;
            }
        }
        let limits = effective_limits(sp, &state.class_limits);
        let layer = sp.size_class.layer();
        if self.world.has_neighbor_within(layer, c, r, limits.min_spacing) {
            return None;
        }

        let id = self.next_id;
        let seed = self.rng.seed();
        let mut plant = Plant::new(id, sp, limits, c, r, &self.world, Rng::new(seed));
        plant.depth_shade = self.depth_shade_for(r);
        let mut cells = Vec::new();
        self.world.footprint(c, r, 0, &mut cells);
        if !self.world.can_claim(layer, &cells, id) {
            return None;
        }
        self.world.claim(layer, &cells, id);
        plant.cells = cells;
        plant.granted_radius_cells = 0;
        self.next_id += 1;
        self.plants.push(plant);
        self.raster_queue.push_back(id);
        Some(self.plants.len() - 1)
    }

    /// Distance haze: plants at the back of the area shade one step lighter,
    /// which stays inside their own ramp instead of tinting them out of palette.
    pub fn depth_shade_for(&self, row: i32) -> f64 {
        let far = if self.world.rows > 1 {
            1.0 - row as f64 / (self.world.rows - 1) as f64
        } else {
            0.0
        };
        far * self.world_cfg.depth_fade
    }

    pub fn plant_index(&self, id: i32) -> Option<usize> {
        self.plants.iter().position(|p| p.id == id)
    }

    /// The same lookup with a slot to try first. A task that comes back to the
    /// same plant every tick almost always finds it where it left it, so the
    /// scan is only paid for on the ticks where something was removed.
    pub fn plant_at(&self, id: i32, hint: usize) -> Option<usize> {
        if self.plants.get(hint).is_some_and(|p| p.id == id) {
            return Some(hint);
        }
        self.plant_index(id)
    }

    pub fn remove_plant_at(&mut self, index: usize) {
        let plant = &self.plants[index];
        let (layer, id) = (plant.layer, plant.id);
        let cells = std::mem::take(&mut self.plants[index].cells);
        self.world.release(layer, &cells, id);
        self.plants.remove(index);
        self.raster_queue.retain(|&q| q != id);
        self.buffer_dirty = true;
    }

    pub fn remove_all(&mut self) {
        self.world.clear();
        self.plants.clear();
        self.raster_queue.clear();
        self.buffer_dirty = true;
    }

    /// Re-rasterizing every growing plant every frame is the expensive part, so
    /// only a fixed number of plants are redrawn per frame; the rest catch up
    /// on later frames.
    pub fn process_raster_queue(&mut self, state: &State, budget: usize) -> usize {
        let mut n = 0;
        while n < budget {
            let id = match self.raster_queue.pop_front() {
                Some(id) => id,
                None => break,
            };
            let index = match self.plant_index(id) {
                Some(i) => i,
                None => continue,
            };
            let species = match state.species.iter().find(|s| s.id == self.plants[index].species_id)
            {
                Some(s) => s,
                None => continue,
            };
            let ramps = self.env.ramps_for(&state.materials, species);
            let env = RasterEnv { shading: &state.shading, ramps: &ramps };
            self.plants[index].raster(&env, &mut self.scratch, species);
            self.buffer_dirty = true;
            n += 1;
        }
        n
    }

    pub fn mark_all_dirty(&mut self) {
        self.raster_queue.clear();
        for p in &mut self.plants {
            p.dirty = true;
            self.raster_queue.push_back(p.id);
        }
        self.buffer_dirty = true;
    }

    /// Painter's algorithm over the area: back rows first, and within a row the
    /// flat items before the standing ones, so nearer plants overlap farther
    /// ones.
    pub fn composite(&mut self, state: &State) {
        let mut buf = std::mem::take(&mut self.buffer);
        self.paint_background(state, &mut buf);
        let shadows = self.world_cfg.shadows;
        for i in self.draw_order() {
            self.blit_plant(&mut buf, i, shadows, Foliage::Solid);
        }
        self.buffer = buf;
        self.buffer_dirty = false;
    }

    /// Back to front order for one frame: rows first, then flat items before
    /// standing ones. The settlement merges its own drawables into this.
    pub fn draw_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.plants.len()).collect();
        order.sort_by(|&a, &b| {
            let pa = &self.plants[a];
            let pb = &self.plants[b];
            pa.row
                .cmp(&pb.row)
                .then(pa.size_class.order().cmp(&pb.size_class.order()))
                .then(pa.id.cmp(&pb.id))
        });
        order
    }

    /// One plant onto a buffer with the world's dimensions, contact shadow
    /// first.
    /// What foliage does where it covers a settler. The default is what a plant
    /// is: opaque, and whoever is behind it is behind it.
    pub fn blit_plant(&self, buf: &mut [u32], index: usize, shadows: bool, over: Foliage) {
        let plant = &self.plants[index];
        let w = &self.world;
        let b = plant.bounds;
        if b.is_empty() {
            return;
        }
        let anchor_x = w.anchor_x(plant.col);
        let anchor_y = w.anchor_y(plant.row);
        if plant.felled > 0.0 {
            self.blit_falling(buf, index, over);
            return;
        }
        if shadows && plant.size_class != SizeClass::Ground && plant.radius_px > 1.0 {
            cast_shadow(w, buf, anchor_x, anchor_y, plant);
        }
        let dx = anchor_x - plant.ox;
        let dy = anchor_y - plant.oy;
        for y in b.y0..=b.y1 {
            let wy = y + dy;
            if wy < 0 || wy >= w.px_h {
                continue;
            }
            let srow = (y * plant.w) as usize;
            let drow = (wy * w.px_w) as usize;
            for x in b.x0..=b.x1 {
                let v = plant.sprite[srow + x as usize];
                if v == 0 {
                    continue;
                }
                let wx = x + dx;
                if wx < 0 || wx >= w.px_w {
                    continue;
                }
                let dst = &mut buf[drow + wx as usize];
                // The mark stays on whatever is drawn over a settler, so the
                // same settler shows through however many leaves are in front.
                if over != Foliage::Solid && crate::util::is_person(*dst) {
                    match over {
                        Foliage::Solid => {}
                        Foliage::Hatched => {
                            if (wx + wy) % 2 != 0 {
                                *dst = crate::util::mark_person(v);
                            }
                        }
                        Foliage::Faded(alpha) => {
                            *dst = crate::util::mark_person(crate::util::mix_packed(*dst, v, alpha));
                        }
                    }
                    continue;
                }
                *dst = v;
            }
        }
    }

    /// A plant on its way over: the same sprite, turned about its foot.
    ///
    /// The destination is walked rather than the source, and each pixel asks
    /// the sprite what was there, because a forward pass would leave the
    /// picture full of holes as it turns. The angle runs as the square of how
    /// far through the fall it is, so a tree leans, then goes.
    fn blit_falling(&self, buf: &mut [u32], index: usize, over: Foliage) {
        let plant = &self.plants[index];
        let w = &self.world;
        let b = plant.bounds;
        let t = clamp(plant.felled, 0.0, 1.0);
        let angle = FALL_ANGLE * t * t * plant.fall_dir();
        let (sin, cos) = angle.sin_cos();
        let anchor_x = w.anchor_x(plant.col);
        let anchor_y = w.anchor_y(plant.row);
        // The box the turned sprite lands in, from its own four corners.
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (cx, cy) in [(b.x0, b.y0), (b.x1, b.y0), (b.x0, b.y1), (b.x1, b.y1)] {
            let u = (cx - plant.ox) as f64;
            let v = (cy - plant.oy) as f64;
            let dx = (u * cos - v * sin).round() as i32;
            let dy = (u * sin + v * cos).round() as i32;
            x0 = x0.min(anchor_x + dx - 1);
            y0 = y0.min(anchor_y + dy - 1);
            x1 = x1.max(anchor_x + dx + 1);
            y1 = y1.max(anchor_y + dy + 1);
        }
        for wy in y0.max(0)..=y1.min(w.px_h - 1) {
            let drow = (wy * w.px_w) as usize;
            let v = (wy - anchor_y) as f64;
            for wx in x0.max(0)..=x1.min(w.px_w - 1) {
                let u = (wx - anchor_x) as f64;
                let sx = (plant.ox as f64 + u * cos + v * sin).round() as i32;
                let sy = (plant.oy as f64 - u * sin + v * cos).round() as i32;
                if sx < b.x0 || sx > b.x1 || sy < b.y0 || sy > b.y1 {
                    continue;
                }
                let value = plant.sprite[(sy * plant.w + sx) as usize];
                if value == 0 {
                    continue;
                }
                let dst = &mut buf[drow + wx as usize];
                if over != Foliage::Solid && crate::util::is_person(*dst) {
                    match over {
                        Foliage::Solid => {}
                        Foliage::Hatched => {
                            if (wx + wy) % 2 != 0 {
                                *dst = crate::util::mark_person(value);
                            }
                        }
                        Foliage::Faded(alpha) => {
                            *dst =
                                crate::util::mark_person(crate::util::mix_packed(*dst, value, alpha));
                        }
                    }
                    continue;
                }
                *dst = value;
            }
        }
    }

    pub fn paint_background(&self, state: &State, buf: &mut [u32]) {
        let w = &self.world;
        let cfg = &self.world_cfg;
        let sky_top = hex_to_packed(&cfg.sky_top);
        let sky_bottom = hex_to_packed(&cfg.sky_bottom);
        for y in 0..w.sky_px {
            let t = if w.sky_px > 1 {
                y as f64 / (w.sky_px - 1) as f64
            } else {
                0.0
            };
            let c = mix_packed(sky_top, sky_bottom, t);
            let row = (y * w.px_w) as usize;
            buf[row..row + w.px_w as usize].fill(c);
        }
        // The ground plane is dithered out of the soil ramp rather than tiled,
        // so the sampler art does not show up as stripes, and lifted toward the
        // light end of the ramp with distance so far rows read as further away.
        let ramp = state.materials.bands(&cfg.soil_sampler);
        let fallback = pack_rgba(52, 38, 28, 255);
        let fade = cfg.depth_fade;
        for y in w.sky_px..w.px_h {
            let row = ((y - w.sky_px) / w.depth_px).min(w.rows - 1);
            let far = if w.rows > 1 {
                1.0 - row as f64 / (w.rows - 1) as f64
            } else {
                0.0
            };
            for x in 0..w.px_w {
                let mut c = fallback;
                if !ramp.is_empty() {
                    let noise = (hash2(x, y, 7331) - 0.5) * 0.24;
                    let t = clamp(0.4 + far * fade * 2.0 + noise, 0.0, 1.0);
                    // The back of the ground plane is the top of it as far as
                    // the box is concerned, so a soil box reads back to front.
                    c = ramp.pick(t, 1.0 - far);
                }
                buf[(y * w.px_w + x) as usize] = c;
            }
        }
    }

    pub fn stats(&self) -> Stats {
        let mut per_species: HashMap<String, usize> = HashMap::new();
        for p in &self.plants {
            *per_species.entry(p.species_id.clone()).or_insert(0) += 1;
        }
        Stats { total: self.plants.len(), per_species, time: self.time, ticks: self.ticks }
    }
}

/// Contact shadow: a foreshortened ellipse under the plant, dithered at the rim
/// so it stays pixel art rather than a soft blob.
pub fn cast_shadow(world: &World, buf: &mut [u32], cx: i32, cy: i32, plant: &Plant) {
    let rx = (plant.radius_px * 0.85).max(2.0);
    let ry = (rx * world.depth_ratio).max(1.0);
    let x0 = ((cx as f64 - rx).floor() as i32).max(0);
    let x1 = ((cx as f64 + rx).ceil() as i32).min(world.px_w - 1);
    // A contact shadow lies on the ground, so it stops at the horizon however
    // far back the thing casting it stands. Clamping to the buffer instead let
    // a plant in the back row throw an ellipse up into the sky.
    let y0 = ((cy as f64 - ry).floor() as i32).max(world.sky_px);
    let y1 = ((cy as f64 + ry).ceil() as i32).min(world.px_h - 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = (x as f64 + 0.5 - cx as f64) / rx;
            let dy = (y as f64 + 0.5 - cy as f64) / ry;
            let d = dx * dx + dy * dy;
            if d > 1.0 {
                continue;
            }
            if d > 0.45 && hash2(x, y, plant.seed as i32) < (d - 0.45) / 0.55 {
                continue;
            }
            let i = (y * world.px_w + x) as usize;
            buf[i] = mix_packed(buf[i], SHADOW_COLOR, 0.42);
        }
    }
}

/// Isolated single plant for the species preview: no neighbors and no grid
/// contention, so the form parameters can be judged on their own.
pub struct Preview {
    pub plant: Plant,
    pub world: World,
}

impl Preview {
    pub fn new(state: &State, species: &Species, seed: u32) -> Self {
        let limits = effective_limits(species, &state.class_limits);
        let cell_px = state.world.cell_px;
        // A private grid large enough that the plant never has to compete for
        // ground, which is what makes the preview show a species on its own.
        let span = limits.max_radius_cells * 2 + 5;
        let cfg = WorldConfig {
            cols: span.max(8),
            rows: span.max(8),
            cell_px,
            depth_px: state.world.depth_px,
            ..state.world.clone()
        };
        let world = World::new(&cfg);
        let col = world.cols / 2;
        let row = world.rows / 2;
        let plant = Plant::new(1, species, limits, col, row, &world, Rng::new(seed));
        Preview { plant, world }
    }

    pub fn grow(&mut self, dt: f64, species: &Species) {
        self.plant.grow(dt, species, &mut self.world, None);
    }

    pub fn raster(&mut self, state: &State, env: &mut Env, scratch: &mut Scratch, species: &Species) {
        let ramps = env.ramps_for(&state.materials, species);
        let raster_env = RasterEnv { shading: &state.shading, ramps: &ramps };
        self.plant.raster(&raster_env, scratch, species);
    }
}

