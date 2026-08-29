//! The settlement simulation.
//!
//! It owns a plant world (the same growth sim the editor tunes), a procedural
//! terrain under it, and the people who live on top of both. Nothing is handed
//! to the settlers: every wall is built out of materials someone carried there,
//! and everything they carry was cut, dug or made somewhere on the map.
//!
//! One map, several towns. A colony keeps its own store, treasury and research;
//! buildings and people carry the id of the town they belong to. When a colony
//! outgrows its ground it sends settlers out to found another one, and the two
//! trade by road and by river from then on.
//!
//! The loop each tick is: grow the wilderness, let people act, run production,
//! settle each colony's economy, sail the boats, then let research and
//! population catch up.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::civ::balloons::{balloons_tick, Balloon};
use crate::civ::boats::{boats_tick, build_boats, Boat};
use crate::civ::buildings::{
    building_by_id, home_rank, scaled_cost, scaled_work, upgrade_of, BuildingDef, Job, Structure,
    BUILDINGS,
};
use crate::civ::civ_render::{composite_settlement, Detail, Item, SpriteCache};
use crate::civ::colony::Colony;
use crate::civ::economy::{run_caravan, stock_targets, update_prices, Sample};
use crate::civ::harvest::{HandCut, Lore};
use crate::civ::names::{inn_name, place_name};
use crate::civ::pathing::PathGrid;
use crate::civ::people::{day_fraction, day_number, daylight, Person, Profession, Traits};
use crate::civ::people_db::PeopleDb;
use crate::civ::phases::{self, Phase};
use crate::civ::planner::{find_site, find_site_near, plan, plan_walls, ring_site};
use crate::civ::resources::{
    add_stock, make_stock, stock_bulk, take_stock, Res, Stock, RES_COUNT, RES_IDS,
};
use crate::civ::social::social_tick;
use crate::civ::tasks::{abandon_task, update_person};
use crate::civ::tech::{tech_by_id, tech_cost, Mods, TechDef, TECHS};
use crate::civ::terrain::{Cell, Terrain};
use crate::plant::Plant;
use crate::rng::Rng;
use crate::sim::Sim;
use crate::species::SizeClass;
use crate::state::State;
use crate::util::{clamp, clamp01, clampi};
use crate::world::World;

/// A rectangle of world pixels. The camera hands one of these in so a frame
/// only pays for what is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub fn whole(world: &World) -> Rect {
        Rect { x0: 0, y0: 0, x1: world.px_w, y1: world.px_h }
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn overlaps(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        x1 > self.x0 && x0 < self.x1 && y1 > self.y0 && y0 < self.y1
    }
}

#[derive(Serialize, Deserialize)]
pub struct Building {
    pub id: i32,
    #[serde(with = "def_ref")]
    pub def: &'static BuildingDef,
    /// The town this belongs to. Everything about stock, wages and research is
    /// answered by that colony rather than by the map.
    pub colony: i32,
    pub col: i32,
    pub row: i32,
    pub w: i32,
    pub h: i32,
    pub built: bool,
    pub cost: Vec<(Res, f64)>,
    pub delivered: Stock,
    pub incoming: Stock,
    pub work: f64,
    pub work_done: f64,
    pub inv: Stock,
    pub out: Stock,
    pub reserved_in: Stock,
    pub reserved_out: Stock,
    pub workers: Vec<u32>,
    pub builders: i32,
    pub craft_progress: f64,
    pub seed: u32,
    pub active: f64,
    pub founded: f64,
    /// Whoever holds the deed, for a home. Zero means the town owns it.
    pub owner: u32,
    /// Who sleeps here. A household is the owner plus their family.
    pub residents: Vec<u32>,
    /// How many people are inside right now, which is what lights the windows.
    pub occupants: i32,
    /// A home being rebuilt one rung larger. Its residents are out in the cold
    /// until it is finished, which is what fills the inns.
    pub upgrading: bool,
    /// Inns are named; everything else goes by its type.
    pub name: Option<String>,
    /// Rooms taken tonight, for an inn.
    pub guests: Vec<u32>,
    /// How wet a farm's fields are, from parched to soaked. Working them dries
    /// them out; damp ground nearby and buckets carried up fill them again.
    /// Nothing but a farm uses it.
    pub water: f64,
    /// Days this home has stood with nobody living in it.
    #[serde(default)]
    pub empty_days: f64,
    /// How far gone it is, from sound at 0 to a heap on the ground at 1. Only
    /// a home ever has one: a workshop nobody is at is between shifts, while a
    /// house nobody lives in is on its way to being a ruin.
    #[serde(default)]
    pub decay: f64,
    /// The watered share of a farm's fields, against the reach it was measured
    /// with. Water does not move, so this holds until the map grows or the
    /// reach setting changes; asking the terrain fresh every tick is a square
    /// search per field cell.
    #[serde(skip)]
    pub soak: Option<(i32, f64)>,
}

/// What a building is, through a save and back: the definition is one of the
/// program's own, so only its name travels.
mod def_ref {
    use crate::civ::buildings::{building_by_id, BuildingDef};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        def: &&'static BuildingDef,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.serialize_str(def.id)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<&'static BuildingDef, D::Error> {
        let id = String::deserialize(d)?;
        building_by_id(&id)
            .ok_or_else(|| serde::de::Error::custom(format!("no building called {id}")))
    }
}

impl Building {
    pub fn out_load(&self) -> f64 {
        self.out.iter().sum()
    }

    pub fn label(&self) -> String {
        match &self.name {
            Some(n) => n.clone(),
            None => self.def.label.to_string(),
        }
    }

    pub fn covers(&self, c: i32, r: i32) -> bool {
        c >= self.col && c < self.col + self.w && r >= self.row && r < self.row + self.h
    }

}

#[derive(Serialize, Deserialize)]
pub struct Pile {
    pub id: i32,
    pub col: i32,
    pub row: i32,
    pub res: Res,
    pub n: f64,
    pub claimed_by: u32,
    pub seed: u32,
    /// Cut by hand rather than dropped by somebody working. A load that was
    /// asked for outranks the work a settler would have chosen for themselves.
    #[serde(default)]
    pub by_hand: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Obituary {
    pub name: String,
    pub age: i32,
    pub cause: String,
    pub day: i32,
    pub colony: i32,
}

/// A cheap copy of the one thing about a plant the settlement asks about: how
/// much of what, and where. Kept in coarse buckets so a camp looking for its
/// next tree reads a few dozen of these instead of every plant on the map.
#[derive(Clone, Copy)]
pub struct PlantMark {
    pub id: i32,
    pub col: i32,
    pub row: i32,
    pub mass: f32,
    /// The picture rather than the cell: how far the plant stands up out of the
    /// ground and how far it spreads either side, in world pixels. What a
    /// pointer aims at is what it can see, and what is drawn round a plant has
    /// to be the shape of the plant.
    pub height_px: f32,
    pub radius_px: f32,
    pub class: SizeClass,
    pub claimed_by: u32,
    /// How much the towns have been taught to want this species, worked out
    /// when the buckets are filled. Carried here rather than looked up per
    /// candidate: every gathering decision reads this for a few hundred marks
    /// and none of them has the species name to look up with.
    pub lore: f32,
}

/// Buckets of plant marks over the map. Rebuilt on a timer rather than kept
/// exact: a camp choosing a tree that grew a little since the last sweep is
/// invisible, and a full rebuild is far cheaper than maintaining it per growth
/// step.
#[derive(Default)]
pub struct PlantIndex {
    pub bucket_cells: i32,
    pub cols: i32,
    pub rows: i32,
    pub buckets: Vec<Vec<PlantMark>>,
    slot_of: HashMap<i32, (usize, usize)>,
    pub(crate) timer: f64,
}

impl PlantIndex {
    const BUCKET: i32 = 8;

    fn resize(&mut self, cols: i32, rows: i32) {
        self.bucket_cells = PlantIndex::BUCKET;
        self.cols = ((cols + PlantIndex::BUCKET - 1) / PlantIndex::BUCKET).max(1);
        self.rows = ((rows + PlantIndex::BUCKET - 1) / PlantIndex::BUCKET).max(1);
        self.buckets = (0..(self.cols * self.rows) as usize).map(|_| Vec::new()).collect();
        self.slot_of.clear();
        self.timer = 0.0;
    }

    fn bucket_of(&self, col: i32, row: i32) -> usize {
        let bc = (col / self.bucket_cells).clamp(0, self.cols - 1);
        let br = (row / self.bucket_cells).clamp(0, self.rows - 1);
        (br * self.cols + bc) as usize
    }

    /// Marks within a radius of a point, as (bucket, slot) pairs the caller can
    /// read straight out of `buckets`.
    pub fn near(&self, col: i32, row: i32, radius: f64, mut f: impl FnMut(&PlantMark)) {
        if self.buckets.is_empty() {
            return;
        }
        let span = (radius / self.bucket_cells as f64).ceil() as i32 + 1;
        let bc = (col / self.bucket_cells).clamp(0, self.cols - 1);
        let br = (row / self.bucket_cells).clamp(0, self.rows - 1);
        for by in (br - span).max(0)..=(br + span).min(self.rows - 1) {
            for bx in (bc - span).max(0)..=(bc + span).min(self.cols - 1) {
                for mark in &self.buckets[(by * self.cols + bx) as usize] {
                    f(mark);
                }
            }
        }
    }

    fn set_claim(&mut self, id: i32, by: u32) {
        if let Some(&(b, s)) = self.slot_of.get(&id) {
            if let Some(mark) = self.buckets.get_mut(b).and_then(|v| v.get_mut(s)) {
                if mark.id == id {
                    mark.claimed_by = by;
                }
            }
        }
    }
}

pub struct ColonyStats {
    pub id: i32,
    pub name: String,
    pub population: usize,
    pub children: usize,
    pub housing: i32,
    pub buildings: usize,
    pub sites: usize,
    pub coin: f64,
    pub food: f64,
    pub known: usize,
    pub happiness: f64,
    pub wealth: f64,
    pub center: (i32, i32),
}

pub struct Stats {
    pub name: String,
    pub day: i32,
    pub day_fraction: f64,
    pub daylight: f64,
    pub population: usize,
    pub children: usize,
    pub professions: Vec<(Profession, usize)>,
    pub housing: i32,
    pub buildings: usize,
    pub sites: usize,
    pub storage: f64,
    pub bulk: f64,
    pub coin: f64,
    pub research: f64,
    pub known: usize,
    pub techs: usize,
    pub births: u32,
    pub deaths: u32,
    pub happiness: f64,
    pub time: f64,
    pub ticks: u64,
    pub colonies: Vec<ColonyStats>,
    pub boats: usize,
}

/// What one step onto a cell costs, given the base cost for the direction.
/// Ground that has been walked over is cheaper, which is how paths wear in;
/// water is dearer by the swimming price, which is why somebody swims a river
/// only when walking round it would be much further.
///
/// A free function because the search borrows the map, and a method would want
/// the settlement at the same time.
/// What the configuration says is in the way, as one number. Zero is nothing:
/// the switch and the threshold are the same question asked twice, and the
/// pathfinder only wants the answer.
fn block_mass(state: &State) -> f64 {
    if state.civ.people.avoid_plants {
        state.civ.people.avoid_mass.max(0.01)
    } else {
        0.0
    }
}

fn step_cost(kind: u8, traffic: f32, swim: f32, base: i32) -> i32 {
    if kind == Cell::Water as u8 {
        return (base as f32 * swim.max(1.0)).round() as i32;
    }
    let worn = (traffic / 6.0).clamp(0.0, 1.0);
    base - (base as f32 * worn * 0.3) as i32
}

pub struct Settlement {
    pub plant_sim: Sim,
    pub terrain: Terrain,
    /// What a step into water costs against a step onto ground, taken from the
    /// configuration each tick. Held here because the pathfinder reads it from
    /// inside a closure that has already borrowed the map.
    pub swim_cost: f32,
    /// How much plant has to be standing in a cell before people walk round it
    /// rather than through it, in cells of mass. Taken from the configuration
    /// each tick, beside the swim cost and for the same reason. Zero lets
    /// everybody through everything.
    pub block_mass: f64,
    pub rng: Rng,
    pub blocked: Vec<u8>,
    pub build_grid: Vec<i32>,
    /// Cells that a building claims but people may still cross: gates, and
    /// nothing else. Kept as its own grid because the pathfinder asks the
    /// question once per neighbor per expanded cell and cannot afford a
    /// building lookup to answer it.
    pub gates: Vec<u8>,
    pub traffic: Vec<f32>,
    /// Cells a standing plant is in the way in. Rebuilt with the coarse plant
    /// index rather than on every growth step: it is read by the pathfinder,
    /// which cannot afford to ask the plant list, and a second stale is a
    /// settler taking one step round a tree that has just come down.
    pub plant_block: Vec<u8>,
    pub paths: PathGrid,
    pub water_paths: PathGrid,
    pub buildings: Vec<Building>,
    building_slot: HashMap<i32, usize>,
    pub next_building_id: i32,
    /// Cut timber and picked food waiting on the ground for somebody to carry
    /// it in. A load bigger than one person can lift becomes one of these.
    pub piles: Vec<Pile>,
    pub next_pile_id: i32,
    pub people: PeopleDb,
    pub colonies: Vec<Colony>,
    pub next_colony_id: i32,
    /// Which colony the panels are reporting on.
    pub focus: usize,
    /// The settler currently held off the map by a pointer, or 0. Somebody
    /// held is skipped by the tick entirely: they keep aging and getting
    /// hungry, but they do not walk, work or take on anything new until they
    /// are put down again.
    pub held: u32,
    /// Plants the pointer is part way through cutting. Not saved: a hold that
    /// was interrupted is not something to come back to.
    pub hand: Vec<HandCut>,
    /// What cutting by hand has taught the gatherers.
    pub lore: Lore,
    pub boats: Vec<Boat>,
    pub next_boat_id: i32,
    /// Canopies in the air over the towns. An experiment, and empty unless one
    /// is switched on.
    pub balloons: Vec<Balloon>,
    pub next_balloon_id: i32,
    pub plant_index: PlantIndex,
    pub time: f64,
    pub day: i32,
    pub ticks: u64,
    pub traffic_timer: f64,
    /// Counts down to the next pass over who is standing near whom.
    pub social_timer: f64,
    pub births: u32,
    pub deaths: u32,
    pub dead: Vec<Obituary>,
    /// The name of the first landing, which is what the world goes by.
    pub name: String,
    pub center: Option<(i32, i32)>,
    pub buffer: Vec<u32>,
    pub bg: Vec<u32>,
    pub bg_key: String,
    pub ground: Vec<u32>,
    pub ground_dirty: bool,
    pub ground_age: u32,
    /// The sampling step the cached ground was painted at. A finer camera needs
    /// the rows this one skipped, so a change forces a rebuild.
    pub ground_step: i32,
    pub buffer_dirty: bool,
    pub warmup_done: f64,
    /// Settlement time at which the last living settler died, or None while
    /// somebody is still going. What the automatic restart counts from.
    pub extinct_at: Option<f64>,
    pub ready: bool,
    pub terrain_version: u32,
    pub sprites: SpriteCache,
    /// The draw list, kept across frames for its capacity alone.
    pub(crate) items: Vec<(i32, i32, i32, Item)>,
    /// What the camera can see, in world pixels. Only this is composited.
    pub view: Rect,
    /// How many world pixels the camera collapses into one on screen. Rows the
    /// upload will not sample are left stale rather than repainted.
    pub px_step: i32,
    /// How much of the drawing is worth doing at the current zoom.
    pub detail: Detail,
    /// Built buildings that keep people well: position, reach, how well, and
    /// whose town. Kept current wherever a building goes up or comes down, so
    /// the sickness check reads a handful of wells rather than sweeping every
    /// building per person per tick.
    health_sources: Vec<(f64, f64, f64, f64, i32)>,
    /// Built lamps: center and lit radius squared, kept the same way.
    light_sources: Vec<(f64, f64, f64)>,
}

impl Settlement {
    pub fn new(state: &State) -> Self {
        let plant_sim = Sim::new(state, state.civ.world.clone());
        let terrain = Terrain::new(&plant_sim.world, &state.civ.terrain, state.civ.seed);
        let n = (plant_sim.world.cols * plant_sim.world.rows) as usize;
        let view = Rect::whole(&plant_sim.world);
        let mut sett = Settlement {
            plant_sim,
            terrain,
            swim_cost: state.civ.people.swim_cost as f32,
            block_mass: block_mass(state),
            rng: Rng::new(state.civ.seed),
            blocked: vec![0; n],
            build_grid: vec![0; n],
            gates: vec![0; n],
            traffic: vec![0.0; n],
            plant_block: vec![0; n],
            paths: PathGrid::default(),
            water_paths: PathGrid::default(),
            buildings: Vec::new(),
            building_slot: HashMap::new(),
            next_building_id: 1,
            piles: Vec::new(),
            next_pile_id: 1,
            people: PeopleDb::new(),
            colonies: Vec::new(),
            next_colony_id: 1,
            focus: 0,
            held: 0,
            hand: Vec::new(),
            lore: Lore::default(),
            boats: Vec::new(),
            next_boat_id: 1,
            balloons: Vec::new(),
            next_balloon_id: 1,
            plant_index: PlantIndex::default(),
            time: 0.0,
            day: 0,
            ticks: 0,
            traffic_timer: 0.0,
            social_timer: 0.0,
            births: 0,
            deaths: 0,
            dead: Vec::new(),
            name: String::new(),
            center: None,
            buffer: Vec::new(),
            bg: Vec::new(),
            bg_key: String::new(),
            ground: Vec::new(),
            ground_dirty: true,
            ground_step: 1,
            ground_age: 0,
            buffer_dirty: true,
            warmup_done: 0.0,
            extinct_at: None,
            ready: false,
            terrain_version: 0,
            sprites: SpriteCache::default(),
            items: Vec::new(),
            view,
            px_step: 1,
            detail: Detail::Full,
            health_sources: Vec::new(),
            light_sources: Vec::new(),
        };
        sett.reset(state, state.civ.seed);
        sett
    }

    pub fn world(&self) -> &World {
        &self.plant_sim.world
    }

    // ---- lifecycle -------------------------------------------------------

    pub fn reset(&mut self, state: &State, seed: u32) {
        let cfg = &state.civ;
        self.rng = Rng::new(seed);
        self.plant_sim.world_cfg = cfg.world.clone();
        self.plant_sim.reset(seed);
        let (cols, rows) = (self.world().cols, self.world().rows);
        let n = (cols * rows) as usize;

        self.plant_sim.wild_scale = cfg.terrain.wildness.max(0.1);
        self.terrain = Terrain::new(self.world(), &cfg.terrain, seed);
        self.blocked = vec![0; n];
        for i in 0..n {
            if self.terrain.kind[i] == Cell::Water as u8 {
                self.blocked[i] = 1;
            }
        }

        self.build_grid = vec![0; n];
        self.gates = vec![0; n];
        self.traffic = vec![0.0; n];
        self.plant_block = vec![0; n];
        self.paths.resize(cols, rows);
        self.water_paths.resize(cols, rows);
        self.plant_index.resize(cols, rows);

        self.buildings.clear();
        self.building_slot.clear();
        self.next_building_id = 1;
        self.piles.clear();
        self.next_pile_id = 1;
        self.hand.clear();
        self.lore.clear();
        self.people.clear();
        self.colonies.clear();
        self.next_colony_id = 1;
        self.focus = 0;
        self.boats.clear();
        self.next_boat_id = 1;
        self.balloons.clear();
        self.next_balloon_id = 1;
        self.time = 0.0;
        self.day = 0;
        self.ticks = 0;
        self.traffic_timer = 0.0;
        self.social_timer = 0.0;
        self.births = 0;
        self.deaths = 0;
        self.dead.clear();
        self.name = String::new();
        self.center = None;
        self.buffer = vec![0; (self.world().px_w * self.world().px_h) as usize];
        self.bg.clear();
        self.bg_key.clear();
        self.ground.clear();
        self.ground_dirty = true;
        self.buffer_dirty = true;
        self.warmup_done = 0.0;
        self.ready = false;
        self.terrain_version += 1;
        self.sprites.clear();
        self.view = Rect::whole(self.world());
    }

    /// Makes the map larger without starting the settlement over.
    ///
    /// The new land goes on the right and along the bottom, so every column and
    /// row that was already there keeps its number and everything standing on
    /// one - buildings, settlers, plants, loads on the ground - stays where it
    /// was. What has to be redone is everything indexed by the width of the
    /// map: each grid is laid out again at the new stride with the old rows
    /// copied across, and every plant claims its cells again in the resized
    /// world.
    ///
    /// The new ground arrives with a wilderness on it rather than bare, warmed
    /// with the same number of seconds a fresh map is, and with the old land
    /// held still while that runs: making the map bigger is not a week passing.
    ///
    /// Says whether anything actually changed.
    pub fn expand(&mut self, state: &State, cols: i32, rows: i32) -> bool {
        let (old_cols, old_rows) = (self.world().cols, self.world().rows);
        let (cols, rows) = (cols.max(old_cols), rows.max(old_rows));
        if cols == old_cols && rows == old_rows {
            return false;
        }
        let n = (cols * rows) as usize;

        // The world grid first: configuring it clears the layer occupancy, so
        // every plant has to be given its cells back afterward.
        self.plant_sim.world_cfg.cols = cols;
        self.plant_sim.world_cfg.rows = rows;
        let world_cfg = self.plant_sim.world_cfg.clone();
        self.plant_sim.world.configure(&world_cfg);
        for i in 0..self.plant_sim.plants.len() {
            let (col, row) = (self.plant_sim.plants[i].col, self.plant_sim.plants[i].row);
            let radius = self.plant_sim.plants[i].granted_radius_cells;
            let (layer, id) = (self.plant_sim.plants[i].layer, self.plant_sim.plants[i].id);
            let mut cells = std::mem::take(&mut self.plant_sim.plants[i].cells);
            self.plant_sim.world.footprint(col, row, radius, &mut cells);
            self.plant_sim.world.claim(layer, &cells, id);
            self.plant_sim.plants[i].cells = cells;
        }
        let px = (self.world().px_w * self.world().px_h) as usize;
        self.plant_sim.buffer = vec![0; px];
        self.plant_sim.buffer_dirty = true;

        let world = self.plant_sim.world.clone();
        self.terrain.expand(&world, &state.civ.terrain);
        // New land can put water within reach of fields that had none.
        for b in &mut self.buildings {
            b.soak = None;
        }

        // Every grid the settlement keeps per cell, at the new stride.
        let mut blocked = vec![0u8; n];
        let mut build_grid = vec![0i32; n];
        let mut gates = vec![0u8; n];
        let mut traffic = vec![0.0f32; n];
        let mut plant_block = vec![0u8; n];
        for r in 0..rows {
            for c in 0..cols {
                let to = (r * cols + c) as usize;
                if c < old_cols && r < old_rows {
                    let from = (r * old_cols + c) as usize;
                    blocked[to] = self.blocked[from];
                    build_grid[to] = self.build_grid[from];
                    gates[to] = self.gates[from];
                    traffic[to] = self.traffic[from];
                    plant_block[to] = self.plant_block[from];
                } else if self.terrain.kind[to] == Cell::Water as u8 {
                    blocked[to] = 1;
                }
            }
        }
        self.blocked = blocked;
        self.build_grid = build_grid;
        self.gates = gates;
        self.traffic = traffic;
        self.plant_block = plant_block;
        self.paths.resize(cols, rows);
        self.water_paths.resize(cols, rows);
        self.plant_index.resize(cols, rows);

        // Grow something on the new ground. The old cells are held: nothing
        // seeds there and nothing already there ages while this runs.
        let warm = state.civ.terrain.warmup.max(0.0);
        if warm > 0.0 {
            let mut held = self.blocked.clone();
            for r in 0..old_rows {
                for c in 0..old_cols {
                    held[(r * cols + c) as usize] = 1;
                }
            }
            let dt = 1.0 / state.civ.sim.tick_hz.max(1.0);
            self.plant_sim.warm_region(state, warm, dt, &held);
            self.plant_sim.process_raster_queue(state, usize::MAX);
        }

        self.buffer = vec![0; px];
        self.bg.clear();
        self.bg_key.clear();
        self.ground.clear();
        self.ground_dirty = true;
        self.buffer_dirty = true;
        self.terrain_version += 1;
        self.view = Rect::whole(self.world());
        self.rebuild_plant_index();
        self.refresh_colonies();
        true
    }

    /// Grows the wilderness before the settlers arrive, then drops the first
    /// storehouse and the founding families next to it. Split out from reset so
    /// the caller can show a note while it runs.
    pub fn bootstrap(&mut self, state: &State) {
        if self.ready {
            return;
        }
        let cfg = &state.civ;
        let warm = cfg.terrain.warmup.max(0.0);
        let dt = 1.0 / cfg.sim.tick_hz.max(1.0);
        let mut t = 0.0;
        while t < warm {
            let blocked = std::mem::take(&mut self.blocked);
            self.plant_sim.step(state, dt, Some(&blocked));
            self.blocked = blocked;
            t += dt;
        }
        self.plant_sim.process_raster_queue(state, usize::MAX);
        self.warmup_done = warm;
        self.rebuild_plant_index();

        let spot = self.terrain.find_start_cell(&mut self.rng);
        let ci = self.found_colony(state, spot, 0);
        self.name = self.colonies[ci].name.clone();
        self.center = Some(spot);

        for res in RES_IDS {
            let n = cfg.start.supplies[res as usize];
            if n > 0.0 {
                add_stock(&mut self.colonies[ci].stock, res, n);
            }
        }
        if cfg.start.storehouse {
            if let Some(def) = building_by_id("storehouse") {
                if let Some(site) = find_site_near(self, state, ci, def, spot.0, spot.1, 6) {
                    self.place_building(state, ci, "storehouse", site.0, site.1, true);
                }
            }
        }

        let pcfg = &cfg.people;
        for _ in 0..cfg.start.population {
            let c = clamp(
                (spot.0 + self.rng.int(-2, 2)) as f64,
                0.0,
                (self.world().cols - 1) as f64,
            ) as i32;
            let r = clamp(
                (spot.1 + self.rng.int(-2, 2)) as f64,
                0.0,
                (self.world().rows - 1) as f64,
            ) as i32;
            let age = self.rng.int(pcfg.adult_age as i32 + 4, 34) as f64;
            let id = self.people.claim_id();
            let mut p = Person::new(id, c, r, age, &mut self.rng);
            p.adult_age = pcfg.adult_age;
            p.lifespan = self.rng.int(pcfg.lifespan_min, pcfg.lifespan_max) as f64;
            p.coin = (cfg.economy.start_coin / cfg.start.population.max(1) as f64).round();
            p.peak_coin = p.coin;
            p.colony = self.colonies[ci].id;
            p.born_in = self.colonies[ci].id;
            p.born = -(age as i32);
            p.log(0, format!("landed at {}", self.colonies[ci].name));
            self.people.insert(p);
        }
        let founding = format!("{} settlers found {}", self.people.count(), self.colonies[ci].name);
        self.colonies[ci].econ.log_event(founding, 0);
        self.refresh_colonies();
        self.match_couples(state, ci);
        self.assign_homes(ci);
        self.refresh_colonies();
        self.assign_workplaces(state, ci);
        self.ready = true;
        self.buffer_dirty = true;
    }

    // ---- colonies --------------------------------------------------------

    pub fn found_colony(&mut self, state: &State, center: (i32, i32), parent: i32) -> usize {
        let id = self.next_colony_id;
        self.next_colony_id += 1;
        let name = place_name(&mut self.rng);
        let seed = self.rng.seed();
        let mut colony = Colony::new(id, name, center, &state.civ.economy, seed);
        colony.parent = parent;
        colony.founded_day = self.day;
        colony.expedition_timer = state.civ.build.expedition_interval;
        self.colonies.push(colony);
        self.refresh_colonies();
        self.colonies.len() - 1
    }

    pub fn colony_index(&self, id: i32) -> Option<usize> {
        self.colonies.iter().position(|c| c.id == id)
    }

    pub fn colony_name(&self, id: i32) -> String {
        self.colony_index(id)
            .map(|i| self.colonies[i].name.clone())
            .unwrap_or_else(|| "nowhere".to_string())
    }

    /// The colony a settler belongs to, as an index. Everybody has one; a
    /// person whose colony has been wound up falls back to the first.
    pub fn colony_of(&self, pi: usize) -> usize {
        self.colony_index(self.people[pi].colony).unwrap_or(0)
    }

    pub fn colony_population(&self, colony: i32) -> usize {
        self.colony_index(colony).map(|i| self.colonies[i].population).unwrap_or(0)
    }

    /// Recomputes the per colony tallies the rest of the sim reads constantly.
    /// One pass over the buildings and one over the people, rather than one of
    /// each per question asked.
    pub fn refresh_colonies(&mut self) {
        for c in &mut self.colonies {
            c.population = 0;
            c.adults = 0;
            c.roofless = 0;
            c.housing = 0;
            c.storage = 0.0;
            c.has_market = false;
            c.stores.clear();
        }
        for (bi, b) in self.buildings.iter().enumerate() {
            if !b.built {
                continue;
            }
            let ci = match self.colonies.iter().position(|c| c.id == b.colony) {
                Some(ci) => ci,
                None => continue,
            };
            let c = &mut self.colonies[ci];
            c.housing += b.def.housing;
            c.storage += b.def.storage;
            c.has_market |= b.def.is_market;
            if b.def.is_store {
                c.stores.push(bi);
            }
        }
        for p in self.people.iter() {
            let ci = match self.colonies.iter().position(|c| c.id == p.colony) {
                Some(ci) => ci,
                None => continue,
            };
            let c = &mut self.colonies[ci];
            c.population += 1;
            if p.age >= p.adult_age {
                c.adults += 1;
                if p.home == 0 {
                    c.roofless += 1;
                }
            }
        }
        // A town that has lost everyone stops being a town. Its buildings stay
        // standing where they are, and the land is free to be settled again.
        for c in &mut self.colonies {
            c.abandoned = c.population == 0;
            if !c.abandoned {
                c.emptied_day = None;
            }
        }
        self.refresh_sources();
    }

    /// Towns anybody still lives in. The planner, the expeditions and the boats
    /// all ask this rather than the colony list, so an emptied town is not
    /// planned for, sailed to or grown into.
    pub fn is_live(&self, ci: usize) -> bool {
        self.colonies.get(ci).is_some_and(|c| !c.abandoned)
    }

    pub fn focus_colony(&self) -> Option<&Colony> {
        self.colonies.get(self.focus.min(self.colonies.len().saturating_sub(1)))
    }

    pub fn mods_of(&self, ci: usize) -> Mods {
        self.colonies.get(ci).map(|c| c.mods).unwrap_or_default()
    }

    // ---- grid helpers ----------------------------------------------------

    pub fn idx(&self, c: i32, r: i32) -> usize {
        (r * self.world().cols + c) as usize
    }

    pub fn in_bounds(&self, c: i32, r: i32) -> bool {
        c >= 0 && c < self.world().cols && r >= 0 && r < self.world().rows
    }

    /// A cell near a spot that somebody can stand on, searched outward in rings
    /// so the answer is the nearest one. Used where a place matters more than a
    /// particular cell of it: the middle of a town is a building or a well as
    /// often as it is open ground.
    pub fn free_spot_near(&self, c: i32, r: i32) -> Option<(i32, i32)> {
        if self.walkable(c, r) {
            return Some((c, r));
        }
        for ring in 1i32..8 {
            for dr in -ring..=ring {
                for dc in -ring..=ring {
                    if dc.abs() != ring && dr.abs() != ring {
                        continue;
                    }
                    if self.walkable(c + dc, r + dr) {
                        return Some((c + dc, r + dr));
                    }
                }
            }
        }
        None
    }

    // ---- settlers in hand ------------------------------------------------

    /// The settler nearest a point on the ground plane, or nothing if none is
    /// within reach of it. Somebody indoors or aboard a boat is not on the map
    /// to be picked up, whatever their recorded position says.
    pub fn person_near(&self, x: f64, y: f64, reach: f64) -> Option<u32> {
        let mut best: Option<(f64, u32)> = None;
        for p in self.people.iter() {
            if p.indoors() || p.aboard != 0 {
                continue;
            }
            // A settler is drawn standing up out of the cell their feet are
            // in, so a point above them is still on them while the same
            // distance below them is only ground.
            let dy = if y < p.y { (p.y - y) * 0.5 } else { y - p.y };
            let d = (p.x - x).powi(2) + dy * dy;
            if d > reach * reach {
                continue;
            }
            match best {
                Some((near, _)) if near <= d => {}
                _ => best = Some((d, p.id)),
            }
        }
        best.map(|(_, id)| id)
    }

    /// Picks a settler up off the map. Whatever they were doing is given up
    /// the way it would be by any other change of plan, so nothing is left
    /// reserved for a job nobody is coming to do.
    pub fn hold_person(&mut self, id: u32) -> bool {
        let pi = match self.people.index_of(id) {
            Some(i) if self.people[i].alive => i,
            _ => return false,
        };
        abandon_task(self, pi);
        let p = &mut self.people[pi];
        p.step_outside();
        p.sleeping = false;
        self.held = id;
        true
    }

    /// Moves whoever is being held. Nothing about the position is checked here:
    /// a settler in hand is off the map, and only where they are put down has
    /// to be somewhere they can be.
    pub fn move_held(&mut self, x: f64, y: f64) {
        let pi = match self.people.index_of(self.held) {
            Some(i) if self.people[i].alive => i,
            // Somebody can die of old age in your hand, and a body is not
            // something to go on dragging about.
            _ => {
                self.held = 0;
                return;
            }
        };
        let p = &mut self.people[pi];
        if x > p.x {
            p.facing = 1;
        } else if x < p.x {
            p.facing = -1;
        }
        p.x = x;
        p.y = y;
    }

    /// Puts the held settler down, and says which cell they landed in. Water
    /// counts as somewhere to land - they swim out of it - but a roof or a
    /// cliff sends them to the nearest cell they can stand in instead.
    pub fn drop_held(&mut self) -> Option<(i32, i32)> {
        let id = std::mem::take(&mut self.held);
        let pi = self.people.index_of(id).filter(|&i| self.people[i].alive)?;
        let (cols, rows) = (self.world().cols, self.world().rows);
        let c = clampi(self.people[pi].x.floor() as i32, 0, cols - 1);
        let r = clampi(self.people[pi].y.floor() as i32, 0, rows - 1);
        let landed = if self.walkable(c, r) || self.in_water(c, r) {
            None
        } else {
            self.free_spot_near(c, r)
        };
        let p = &mut self.people[pi];
        match landed {
            Some((fc, fr)) => {
                p.x = fc as f64 + 0.5;
                p.y = fr as f64 + 0.5;
            }
            // Where they were let go of, kept as it was so nothing jumps on
            // release, but never off the edge of the map.
            None => {
                p.x = p.x.clamp(0.0, cols as f64 - 0.01);
                p.y = p.y.clamp(0.0, rows as f64 - 0.01);
            }
        }
        // They plan again from where they are standing rather than carrying on
        // to somewhere they were walking from the other side of the map.
        p.clear_task();
        let p = &self.people[pi];
        Some((p.cell_col(), p.cell_row()))
    }

    /// Whether a cell is water, which is where somebody is swimming rather than
    /// walking.
    pub fn in_water(&self, c: i32, r: i32) -> bool {
        self.in_bounds(c, r) && self.terrain.kind[self.idx(c, r)] == Cell::Water as u8
    }

    /// Ground somebody can stand on and work from. Water is crossable but is
    /// not somewhere anything happens, so this stays dry land.
    pub fn walkable(&self, c: i32, r: i32) -> bool {
        if !self.in_bounds(c, r) {
            return false;
        }
        let i = self.idx(c, r);
        self.terrain.kind[i] != Cell::Water as u8
            && (self.build_grid[i] == 0 || self.gates[i] != 0)
    }

    pub fn building_at(&self, c: i32, r: i32) -> Option<usize> {
        if !self.in_bounds(c, r) {
            return None;
        }
        let id = self.build_grid[self.idx(c, r)];
        if id == 0 {
            None
        } else {
            self.building_index(id)
        }
    }

    /// O(1): buildings are few and rarely removed, so the slot map is kept
    /// exact rather than scanned for.
    pub fn building_index(&self, id: i32) -> Option<usize> {
        self.building_slot.get(&id).copied()
    }

    pub fn reindex_buildings(&mut self) {
        self.building_slot.clear();
        for (i, b) in self.buildings.iter().enumerate() {
            self.building_slot.insert(b.id, i);
        }
    }

    pub fn person_index(&self, id: u32) -> Option<usize> {
        self.people.index_of(id)
    }

    pub fn pile_index(&self, id: i32) -> Option<usize> {
        self.piles.iter().position(|q| q.id == id)
    }

    /// Free cells touching the building, which is where people stand to use it.
    /// A cell whose only neighbors are diagonal is a trap: the path finder does
    /// not cut corners, so a spot wedged between a wall and the water can be
    /// walkable and still unreachable. Those sort to the back.
    pub fn access_cells(&self, bi: usize) -> Vec<(i32, i32)> {
        let b = &self.buildings[bi];
        let mut cands: Vec<(i32, i32, i32)> = Vec::new();
        for r in b.row - 1..=b.row + b.h {
            for c in b.col - 1..=b.col + b.w {
                let inside = c >= b.col && c < b.col + b.w && r >= b.row && r < b.row + b.h;
                if inside || !self.walkable(c, r) {
                    continue;
                }
                let mut open = 0;
                if self.walkable(c + 1, r) {
                    open += 1;
                }
                if self.walkable(c - 1, r) {
                    open += 1;
                }
                if self.walkable(c, r + 1) {
                    open += 1;
                }
                if self.walkable(c, r - 1) {
                    open += 1;
                }
                cands.push((c, r, open));
            }
        }
        // Reachable first, then the near side, so workers stand in front.
        cands.sort_by(|a, z| {
            (z.2 > 0)
                .cmp(&(a.2 > 0))
                .then(z.1.cmp(&a.1))
                .then(z.2.cmp(&a.2))
        });
        if cands.is_empty() {
            return vec![(b.col, b.row + b.h)];
        }
        cands.into_iter().map(|(c, r, _)| (c, r)).collect()
    }

    pub fn access_cell(&self, bi: usize) -> (i32, i32) {
        self.access_cells(bi)[0]
    }

    /// A route over ground, or through water at a price. The grid is taken out
    /// of the settlement for the search because the passability test reads the
    /// map.
    pub fn find_path(&mut self, sc: i32, sr: i32, tc: i32, tr: i32) -> Option<Vec<(i32, i32)>> {
        let swim_cost = self.swim_cost;
        let cols = self.world().cols;
        let rows = self.world().rows;
        if !self.paths.matches(cols, rows) {
            self.paths.resize(cols, rows);
        }
        let mut grid = std::mem::take(&mut self.paths);
        let kind = &self.terrain.kind;
        let build = &self.build_grid;
        let gates = &self.gates;
        let traffic = &self.traffic;
        let plants = &self.plant_block;
        let passable = |c: i32, r: i32| {
            if c < 0 || c >= cols || r < 0 || r >= rows {
                return false;
            }
            let i = (r * cols + c) as usize;
            (build[i] == 0 || gates[i] != 0) && plants[i] == 0
        };
        let swim = swim_cost;
        let cost = |i: usize, base: i32| step_cost(kind[i], traffic[i], swim, base);
        let out = grid.find((sc, sr), (tc, tr), 24_000, passable, cost);
        self.paths = grid;
        out
    }

    /// Whether a route from one place to another survives shutting one more
    /// cell. This is the whole safety rule behind a wall: a piece that would
    /// leave nobody a way through is never queued, so a ring closes down to
    /// its gates and never past them.
    pub fn path_exists_without(
        &mut self,
        shut: (i32, i32),
        from: (i32, i32),
        to: (i32, i32),
    ) -> bool {
        let cols = self.world().cols;
        let rows = self.world().rows;
        if !self.paths.matches(cols, rows) {
            self.paths.resize(cols, rows);
        }
        let mut grid = std::mem::take(&mut self.paths);
        let kind = &self.terrain.kind;
        let build = &self.build_grid;
        let gates = &self.gates;
        let traffic = &self.traffic;
        // Dry land only, unlike the route search. This is the rule that stops a
        // wall being closed on the last way out, and a way out is a gate to
        // walk through: a town whose only exit is a swim is walled in as far as
        // anybody living in it is concerned.
        let passable = |c: i32, r: i32| {
            if c < 0 || c >= cols || r < 0 || r >= rows || (c, r) == shut {
                return false;
            }
            let i = (r * cols + c) as usize;
            kind[i] != Cell::Water as u8 && (build[i] == 0 || gates[i] != 0)
        };
        let cost = |i: usize, base: i32| step_cost(Cell::Grass as u8, traffic[i], 1.0, base);
        let out = grid.find(from, to, 24_000, passable, cost);
        self.paths = grid;
        out.is_some()
    }

    // ---- buildings -------------------------------------------------------

    pub fn store_capacity(&self, colony: i32) -> f64 {
        self.colony_index(colony).map(|i| self.colonies[i].storage).unwrap_or(0.0)
    }

    pub fn store_space(&self, ci: usize) -> f64 {
        let colony = self.colonies[ci].id;
        (self.store_capacity(colony) - stock_bulk(&self.colonies[ci].stock)).max(0.0)
    }

    pub fn nearest_store(&self, colony: i32, col: i32, row: i32) -> Option<usize> {
        let ci = self.colony_index(colony)?;
        let mut best = None;
        let mut best_d = f64::INFINITY;
        for &i in &self.colonies[ci].stores {
            let b = &self.buildings[i];
            let dx = (b.col - col) as f64;
            let dy = (b.row - row) as f64;
            let d = dx * dx + dy * dy;
            if d < best_d {
                best = Some(i);
                best_d = d;
            }
        }
        best
    }

    pub fn has_market(&self, colony: i32) -> bool {
        self.colony_index(colony).is_some_and(|i| self.colonies[i].has_market)
    }

    pub fn count_built(&self, colony: i32, type_id: &str) -> usize {
        self.buildings
            .iter()
            .filter(|b| b.def.id == type_id && b.built && b.colony == colony)
            .count()
    }

    pub fn count_all(&self, colony: i32, type_id: &str) -> usize {
        self.buildings
            .iter()
            .filter(|b| b.def.id == type_id && b.colony == colony)
            .count()
    }

    pub fn sites(&self) -> Vec<usize> {
        (0..self.buildings.len()).filter(|&i| !self.buildings[i].built).collect()
    }

    pub fn colony_sites(&self, colony: i32) -> usize {
        self.buildings.iter().filter(|b| !b.built && b.colony == colony).count()
    }

    pub fn housing_capacity(&self, colony: i32) -> i32 {
        self.colony_index(colony).map(|i| self.colonies[i].housing).unwrap_or(0)
    }

    pub fn work_slots(&self, colony: i32, type_id: &str) -> i32 {
        let mut open = 0;
        for b in &self.buildings {
            if b.built && b.def.id == type_id && b.colony == colony {
                open += b.def.slots as i32 - b.workers.len() as i32;
            }
        }
        open
    }

    pub fn place_building(
        &mut self,
        state: &State,
        ci: usize,
        type_id: &str,
        col: i32,
        row: i32,
        instant: bool,
    ) -> Option<usize> {
        let def = building_by_id(type_id)?;
        let cost = scaled_cost(def, &state.civ.build);
        let id = self.next_building_id;
        self.next_building_id += 1;
        let name = if def.is_inn { Some(inn_name(&mut self.rng)) } else { None };
        let b = Building {
            id,
            def,
            colony: self.colonies[ci].id,
            col,
            row,
            w: def.w,
            h: def.h,
            built: false,
            cost: cost.clone(),
            delivered: [0.0; RES_COUNT],
            incoming: [0.0; RES_COUNT],
            work: scaled_work(def, &state.civ.build),
            work_done: 0.0,
            inv: [0.0; RES_COUNT],
            out: [0.0; RES_COUNT],
            reserved_in: [0.0; RES_COUNT],
            reserved_out: [0.0; RES_COUNT],
            workers: Vec::new(),
            builders: 0,
            craft_progress: 0.0,
            seed: self.rng.seed(),
            active: 0.0,
            founded: self.time,
            owner: 0,
            residents: Vec::new(),
            occupants: 0,
            upgrading: false,
            name,
            guests: Vec::new(),
            water: 1.0,
            empty_days: 0.0,
            decay: 0.0,
            soak: None,
        };
        self.buildings.push(b);
        let bi = self.buildings.len() - 1;
        self.building_slot.insert(id, bi);
        self.claim_footprint(bi);
        self.clear_plants_under(state, ci, bi);
        self.ground_dirty = true;
        if instant {
            for &(res, n) in &cost {
                self.buildings[bi].delivered[res as usize] = n;
            }
            self.buildings[bi].work_done = self.buildings[bi].work;
            self.finish_building(state, bi);
        }
        self.buffer_dirty = true;
        Some(bi)
    }

    fn claim_footprint(&mut self, bi: usize) {
        let (id, col, row, w, h) = {
            let b = &self.buildings[bi];
            (b.id, b.col, b.row, b.w, b.h)
        };
        for r in row..row + h {
            for c in col..col + w {
                if !self.in_bounds(c, r) {
                    continue;
                }
                let i = self.idx(c, r);
                self.build_grid[i] = id;
                self.blocked[i] = 1;
            }
        }
    }

    fn release_footprint(&mut self, id: i32, col: i32, row: i32, w: i32, h: i32) {
        for r in row..row + h {
            for c in col..col + w {
                if !self.in_bounds(c, r) {
                    continue;
                }
                let i = self.idx(c, r);
                if self.build_grid[i] != id {
                    continue;
                }
                self.build_grid[i] = 0;
                self.gates[i] = 0;
                if self.terrain.kind[i] != Cell::Water as u8 {
                    self.blocked[i] = 0;
                }
            }
        }
    }

    /// Ground is cleared before anything is raised on it; half the timber of
    /// whatever stood there goes into the store.
    fn clear_plants_under(&mut self, state: &State, ci: usize, bi: usize) {
        let (col, row, w, h) = {
            let b = &self.buildings[bi];
            (b.col, b.row, b.w, b.h)
        };
        let share = state.civ.work.clear_yield;
        for i in (0..self.plant_sim.plants.len()).rev() {
            let p = &self.plant_sim.plants[i];
            if p.col >= col && p.col < col + w && p.row >= row && p.row < row + h {
                let mass = self.plant_mass(p);
                let woody = matches!(
                    p.size_class,
                    crate::species::SizeClass::Tree | crate::species::SizeClass::Shrub
                );
                if woody {
                    self.deposit(state, ci, Res::Wood, (mass * share).round(), None);
                } else {
                    self.deposit(state, ci, Res::Fiber, (mass * share * 0.5).round(), None);
                }
                self.plant_sim.remove_plant_at(i);
            }
        }
    }

    pub fn finish_building(&mut self, state: &State, bi: usize) {
        self.buildings[bi].built = true;
        self.buildings[bi].upgrading = false;
        self.buildings[bi].work_done = self.buildings[bi].work;
        // A gateway is only a way through once the gate is actually standing;
        // while it is scaffolding it is as shut as the wall beside it.
        if self.buildings[bi].def.structure.passable() {
            let (col, row, w, h) = {
                let b = &self.buildings[bi];
                (b.col, b.row, b.w, b.h)
            };
            for r in row..row + h {
                for c in col..col + w {
                    if self.in_bounds(c, r) {
                        let i = self.idx(c, r);
                        self.gates[i] = 1;
                    }
                }
            }
        }
        self.ground_dirty = true;
        self.refresh_colonies();
        let label = self.buildings[bi].label();
        let colony = self.buildings[bi].colony;
        let day = self.day;
        if let Some(ci) = self.colony_index(colony) {
            self.colonies[ci].econ.log_event(format!("{label} finished"), day);
            self.assign_workplaces(state, ci);
            if self.buildings[bi].def.housing > 0 {
                self.assign_homes(ci);
            }
        }
        self.buffer_dirty = true;
    }

    pub fn remove_building(&mut self, bi: usize) {
        let (id, col, row, w, h) = {
            let b = &self.buildings[bi];
            (b.id, b.col, b.row, b.w, b.h)
        };
        self.buildings.remove(bi);
        self.reindex_buildings();
        self.release_footprint(id, col, row, w, h);
        self.refresh_colonies();
        let mut abandon = Vec::new();
        for pi in self.people.live_indices() {
            let p = &mut self.people[pi];
            if p.work == id {
                p.work = 0;
            }
            if p.home == id {
                p.home = 0;
            }
            if p.owns == id {
                p.owns = 0;
            }
            if p.stall == id {
                p.stall = 0;
            }
            if p.inside == id {
                p.inside = 0;
            }
            if p.task.as_ref().is_some_and(|t| t.touches_building(id)) {
                abandon.push(pi);
            }
        }
        for pi in abandon {
            abandon_task(self, pi);
        }
        self.buffer_dirty = true;
    }

    // ---- homes -----------------------------------------------------------

    /// A settler steps inside. The door is where they are shown to be, and the
    /// window lights follow the count.
    pub fn enter_building(&mut self, pi: usize, bi: usize) {
        let id = self.buildings[bi].id;
        if self.people[pi].inside == id {
            return;
        }
        self.leave_building(pi);
        // Parked on the doorstep rather than inside the walls: nobody indoors
        // is drawn, and standing on the building's own blocked cells would mean
        // every walk out of it started from unwalkable ground.
        let at = self.access_cell(bi);
        self.people[pi].inside = id;
        self.people[pi].x = at.0 as f64 + 0.5;
        self.people[pi].y = at.1 as f64 + 0.5;
        self.people[pi].path.clear();
        self.people[pi].path_at = 0;
        self.buildings[bi].occupants += 1;
        self.buffer_dirty = true;
    }

    pub fn leave_building(&mut self, pi: usize) {
        let id = self.people[pi].inside;
        if id == 0 {
            return;
        }
        self.people[pi].inside = 0;
        if let Some(bi) = self.building_index(id) {
            self.buildings[bi].occupants = (self.buildings[bi].occupants - 1).max(0);
        }
        self.buffer_dirty = true;
    }

    /// The first adult under an unowned roof takes the deed, whether they got
    /// there on their own or by moving in with family. This is what makes an
    /// upgrade somebody's decision later, and it is why a town of huts ends up
    /// with a named owner per hut rather than with a municipal housing stock.
    ///
    /// Nobody takes a second deed: a settler sleeping elsewhere because their
    /// own house is scaffolding still owns that house.
    fn claim_deed(&mut self, bi: usize, pi: usize) {
        if self.buildings[bi].owner != 0
            || self.people[pi].owns != 0
            || !self.people[pi].adult()
        {
            return;
        }
        let person_id = self.people[pi].id;
        let id = self.buildings[bi].id;
        self.buildings[bi].owner = person_id;
        self.people[pi].owns = id;
        let day = self.day;
        let what = self.buildings[bi].label();
        self.people[pi].log(day, format!("took the deed to a {}", what.to_lowercase()));
    }

    /// Gives up a deed, writing both ends. Leaving a town, or being buried,
    /// goes through here; without it the house keeps pointing at somebody who
    /// is no longer in it, and the next household pass re-links them to a
    /// building in a colony they walked away from.
    /// Gives up a stall. The counter stays where it is with nobody behind it,
    /// which is what lets the next settler with the coin take it over rather
    /// than pay for another one.
    fn release_stall(&mut self, pi: usize) {
        let stall = self.people[pi].stall;
        if stall == 0 {
            return;
        }
        self.people[pi].stall = 0;
        if let Some(bi) = self.building_index(stall) {
            if self.buildings[bi].owner == self.people[pi].id {
                self.buildings[bi].owner = 0;
                self.buildings[bi].workers.clear();
            }
        }
    }

    fn release_deed(&mut self, pi: usize) {
        let owns = self.people[pi].owns;
        if owns == 0 {
            return;
        }
        self.people[pi].owns = 0;
        if let Some(bi) = self.building_index(owns) {
            if self.buildings[bi].owner == self.people[pi].id {
                self.buildings[bi].owner = 0;
            }
        }
    }

    /// Rebuilds one colony's households. Deeds are not touched: an owner keeps
    /// their house until they die, and a household follows its owner.
    pub fn assign_homes(&mut self, ci: usize) {
        let colony = self.colonies[ci].id;
        let homes: Vec<(usize, i32, i32)> = (0..self.buildings.len())
            .filter(|&i| {
                self.buildings[i].built
                    && self.buildings[i].colony == colony
                    && self.buildings[i].def.housing > 0
            })
            .map(|i| (i, self.buildings[i].id, self.buildings[i].def.housing))
            .collect();
        for &(bi, _, _) in &homes {
            self.buildings[bi].residents.clear();
        }
        // Deeds are settled before beds are, and over every home in the colony
        // rather than only the finished ones: a house being rebuilt one rung
        // larger is still its owner's house, they just cannot sleep in it. The
        // two sides of the deed are written here together so they cannot drift
        // apart.
        for bi in 0..self.buildings.len() {
            let b = &self.buildings[bi];
            if b.colony != colony || b.def.housing == 0 {
                continue;
            }
            let owner = b.owner;
            if owner == 0 {
                continue;
            }
            let id = b.id;
            match self.people.index_of(owner) {
                Some(pi) if self.people[pi].alive => self.people[pi].owns = id,
                _ => self.buildings[bi].owner = 0,
            }
        }

        let mut used: HashMap<i32, i32> = homes.iter().map(|&(_, id, _)| (id, 0)).collect();
        let residents = |used: &HashMap<i32, i32>, id: i32| used.get(&id).copied().unwrap_or(0);
        let people: Vec<usize> = self
            .people
            .live_indices()
            .into_iter()
            .filter(|&pi| self.people[pi].colony == colony)
            .collect();

        // Owners first, then whoever already lived somewhere, then the rest.
        let mut order = people.clone();
        order.sort_by_key(|&pi| {
            let p = &self.people[pi];
            (p.owns == 0) as i32 * 2 + (p.home == 0) as i32
        });

        for &pi in &order {
            let owns = self.people[pi].owns;
            if owns != 0 {
                match self.building_index(owns) {
                    // Their own roof, if it is standing. While it is a site
                    // they keep the deed and fall through to a rented bed.
                    Some(bi) if self.buildings[bi].built => {
                        self.people[pi].home = owns;
                        *used.entry(owns).or_insert(0) += 1;
                        self.buildings[bi].residents.push(self.people[pi].id);
                        continue;
                    }
                    Some(_) => {}
                    None => self.people[pi].owns = 0,
                }
            }
            // Family sticks together: a spouse's roof, then a parent's.
            let kin = [
                self.people[pi].spouse,
                self.people[pi].mother,
                self.people[pi].father,
            ];
            let mut placed = false;
            for id in kin {
                if id == 0 {
                    continue;
                }
                let home = match self.people.get(id) {
                    Some(k) if k.alive => k.home,
                    _ => continue,
                };
                if home == 0 {
                    continue;
                }
                let cap = homes.iter().find(|&&(_, hid, _)| hid == home).map(|&(_, _, c)| c);
                let cap = match cap {
                    Some(c) => c,
                    None => continue,
                };
                if residents(&used, home) >= cap {
                    continue;
                }
                self.people[pi].home = home;
                *used.entry(home).or_insert(0) += 1;
                if let Some(bi) = self.building_index(home) {
                    self.buildings[bi].residents.push(self.people[pi].id);
                    self.claim_deed(bi, pi);
                }
                placed = true;
                break;
            }
            if placed {
                continue;
            }

            self.people[pi].home = 0;
            for &(bi, id, cap) in &homes {
                if residents(&used, id) >= cap {
                    continue;
                }
                self.people[pi].home = id;
                *used.entry(id).or_insert(0) += 1;
                let person_id = self.people[pi].id;
                self.buildings[bi].residents.push(person_id);
                self.claim_deed(bi, pi);
                break;
            }
        }
    }

    /// A settler with the coin for the next rung of house has it rebuilt over
    /// their own footprint. The coin goes to the colony treasury, which is what
    /// then pays the laborers who carry the brick.
    fn upgrade_homes(&mut self, state: &State, ci: usize) {
        let cfg = &state.civ.build;
        if !cfg.home_upgrades {
            return;
        }
        let colony = self.colonies[ci].id;
        // A town does not put every roof it has on the ground at once. Pulling
        // a house down takes its beds out of the housing stock and its
        // household out of their own home, so rebuilds are rationed and only
        // start when there is somewhere else for those people to sleep.
        let under_way = self
            .buildings
            .iter()
            .filter(|b| b.colony == colony && b.upgrading)
            .count() as i32;
        if under_way >= cfg.max_home_rebuilds.max(0) {
            return;
        }
        let spare_beds = self.colonies[ci].housing - self.colonies[ci].population as i32;
        let inn_rooms: i32 = self
            .buildings
            .iter()
            .filter(|b| b.built && b.def.is_inn && b.colony == colony)
            .map(|b| b.def.rooms - b.guests.len() as i32)
            .sum();
        let candidates: Vec<usize> = (0..self.buildings.len())
            .filter(|&bi| {
                let b = &self.buildings[bi];
                b.built && !b.upgrading && b.colony == colony && b.owner != 0 && b.def.upgrade_to.is_some()
            })
            .collect();
        for bi in candidates {
            let household = self.buildings[bi].residents.len() as i32;
            if spare_beds - household < 0 && inn_rooms < household {
                continue;
            }
            let def = self.buildings[bi].def;
            let next = match upgrade_of(def) {
                Some(n) => n,
                None => continue,
            };
            if !next.base && !self.colonies[ci].unlocked.contains(next.id) {
                continue;
            }
            let owner = self.buildings[bi].owner;
            let pi = match self.people.index_of(owner) {
                Some(pi) if self.people[pi].alive => pi,
                _ => continue,
            };
            // A thrifty settler commits sooner; a spendthrift waits until the
            // coin is embarrassing.
            let price = def.upgrade_coin * (1.45 - self.people[pi].traits.thrift * 0.55)
                * state.civ.build.upgrade_scale;
            if self.people[pi].coin < price {
                continue;
            }
            let anchor = match self.upgrade_anchor(state, bi, next) {
                Some(a) => a,
                None => continue,
            };
            let paid = self.people[pi].spend(price);
            self.colonies[ci].econ.coin += paid;
            let day = self.day;
            let name = self.people[pi].name.clone();
            self.people[pi].log(day, format!("paid for a {}", next.label.to_lowercase()));
            self.people[pi].owns = self.buildings[bi].id;
            self.start_upgrade(state, bi, next, anchor);
            let label = next.label;
            self.colonies[ci]
                .econ
                .log_event(format!("{name} commissioned a {}", label.to_lowercase()), day);
            return;
        }
    }

    /// A settler who has come to dread the dark, and has the coin for it, pays
    /// for a lamp post outside their own house.
    ///
    /// The cost is the same for everybody, which is the whole point: it is the
    /// rich who light their street first, not the frightened. Fear is what
    /// makes somebody want one; coin is what lets them have it.
    fn light_by_fear(&mut self, state: &State, ci: usize) {
        let cfg = &state.civ.build;
        if !cfg.lamps_by_fear {
            return;
        }
        let lamp = match building_by_id("lamp") {
            Some(d) => d,
            None => return,
        };
        if !lamp.base && !self.colonies[ci].unlocked.contains(lamp.id) {
            return;
        }
        let colony = self.colonies[ci].id;
        let want = state.civ.people.fear_to_light;
        let price = cfg.lamp_coin * cfg.upgrade_scale;

        let mut candidates: Vec<usize> = self
            .people
            .live_indices()
            .into_iter()
            .filter(|&pi| {
                let p = &self.people[pi];
                // Whoever sleeps under the roof, not only whoever holds the
                // deed: the settlers the dark actually gets to are the ones
                // walking home to somebody else's house.
                p.colony == colony && p.home != 0 && p.fear >= want && p.coin >= price
            })
            .collect();
        // The most frightened first, so a town that can only afford one post
        // puts it where it is wanted most.
        candidates.sort_by(|a, b| self.people[*b].fear.total_cmp(&self.people[*a].fear));

        for pi in candidates {
            let home = match self.building_index(self.people[pi].home) {
                Some(bi) if self.buildings[bi].built => bi,
                _ => continue,
            };
            // A house already on a lit street needs no second lamp, and the
            // fear it was raised against goes with it.
            let (hc, hr) = (self.buildings[home].col, self.buildings[home].row);
            if self.lit_at(hc, hr) {
                self.people[pi].fear = 0.0;
                continue;
            }
            let at = match self.lamp_spot(state, home, lamp) {
                Some(a) => a,
                None => continue,
            };
            if self.place_building(state, ci, lamp.id, at.0, at.1, false).is_none() {
                continue;
            }
            let paid = self.people[pi].spend(price);
            self.colonies[ci].econ.coin += paid;
            let day = self.day;
            let name = self.people[pi].name.clone();
            self.people[pi].fear = 0.0;
            self.people[pi].log(day, "paid for a lamp post outside".to_string());
            self.colonies[ci]
                .econ
                .log_event(format!("{name} paid for a lamp post"), day);
            return;
        }
    }

    /// Somewhere just outside a house to stand a lamp. Close in first, so the
    /// post lights the door rather than the field behind it.
    fn lamp_spot(&self, state: &State, home: usize, lamp: &BuildingDef) -> Option<(i32, i32)> {
        let (col, row, w, h) = {
            let b = &self.buildings[home];
            (b.col, b.row, b.w, b.h)
        };
        let _ = state;
        for ring in 1..=3 {
            for dr in -ring..=h - 1 + ring {
                for dc in -ring..=w - 1 + ring {
                    let outside = dc < 0 || dr < 0 || dc >= w || dr >= h;
                    let on_ring = dc == -ring || dr == -ring || dc == w - 1 + ring || dr == h - 1 + ring;
                    if !outside || !on_ring {
                        continue;
                    }
                    let (c, r) = (col + dc, row + dr);
                    if crate::civ::planner::fits(self, lamp, c, r, 0, 0) {
                        return Some((c, r));
                    }
                }
            }
        }
        None
    }

    /// Where the bigger house can stand. The old footprint is preferred, then
    /// the shifts that keep a corner of it, so a manor grows out of the house
    /// that was there rather than appearing across the road.
    fn upgrade_anchor(&self, state: &State, bi: usize, next: &BuildingDef) -> Option<(i32, i32)> {
        let (col, row, id) = {
            let b = &self.buildings[bi];
            (b.col, b.row, b.id)
        };
        let mut tries = vec![(col, row)];
        for dc in -(next.w - 1).max(0)..=0 {
            for dr in -(next.h - 1).max(0)..=0 {
                if dc != 0 || dr != 0 {
                    tries.push((col + dc, row + dr));
                }
            }
        }
        // A bigger house grows into its own yard, so the gap the planner keeps
        // between new buildings is not applied to a rebuild. Without this a
        // hut in a finished street can never become anything.
        let _ = state;
        tries
            .into_iter()
            .find(|&(c, r)| crate::civ::planner::fits(self, next, c, r, id, 0))
    }

    fn start_upgrade(&mut self, state: &State, bi: usize, next: &'static BuildingDef, at: (i32, i32)) {
        let (id, col, row, w, h) = {
            let b = &self.buildings[bi];
            (b.id, b.col, b.row, b.w, b.h)
        };
        self.release_footprint(id, col, row, w, h);
        // Salvage: the old walls come down into the site, so an upgrade is not
        // the full price of the new house.
        let salvage = state.civ.build.upgrade_salvage;
        let old_cost = self.buildings[bi].cost.clone();
        let cost = scaled_cost(next, &state.civ.build);
        {
            let b = &mut self.buildings[bi];
            b.def = next;
            b.col = at.0;
            b.row = at.1;
            b.w = next.w;
            b.h = next.h;
            b.built = false;
            b.upgrading = true;
            b.work = scaled_work(next, &state.civ.build);
            b.work_done = 0.0;
            b.cost = cost;
            b.delivered = [0.0; RES_COUNT];
            b.incoming = [0.0; RES_COUNT];
            b.builders = 0;
            b.craft_progress = 0.0;
            b.occupants = 0;
            b.soak = None;
            for &(res, n) in &old_cost {
                b.delivered[res as usize] += (n * salvage).floor();
            }
            for &(res, need) in b.cost.clone().iter() {
                b.delivered[res as usize] = b.delivered[res as usize].min(need);
            }
        }
        self.claim_footprint(bi);
        self.refresh_colonies();
        let colony = self.buildings[bi].colony;
        if let Some(ci) = self.colony_index(colony) {
            self.clear_plants_under(state, ci, bi);
        }
        // Everyone under that roof is out until it is finished.
        let residents = self.buildings[bi].residents.clone();
        for rid in residents {
            if let Some(pi) = self.people.index_of(rid) {
                if self.people[pi].inside == id {
                    self.leave_building(pi);
                }
            }
        }
        self.ground_dirty = true;
        self.buffer_dirty = true;
    }

    // ---- stalls ----------------------------------------------------------

    /// Where a town's trade happens: its market if it has one, its center
    /// otherwise. Stalls cluster around this rather than being scattered.
    pub fn market_cell(&self, colony: i32) -> (i32, i32) {
        for b in &self.buildings {
            if b.built && b.def.is_market && b.colony == colony {
                return (b.col, b.row);
            }
        }
        self.colony_index(colony)
            .map(|ci| self.colonies[ci].center)
            .unwrap_or((0, 0))
    }

    /// Somebody opens a stall, or takes over one that has been standing empty.
    ///
    /// This has the same shape as a house rebuild and for the same reason: it
    /// is one settler's decision and one settler's coin. The price goes into
    /// the treasury, which is what then pays the laborers who carry the timber
    /// out to the site. Nobody is ever assigned to keep a stall - the person
    /// who paid for it stands behind it.
    fn open_stalls(&mut self, state: &State, ci: usize) {
        let cfg = &state.civ.build;
        if !cfg.stalls {
            return;
        }
        let def = match building_by_id("stall") {
            Some(d) => d,
            None => return,
        };
        if !def.base && !self.colonies[ci].unlocked.contains(def.id) {
            return;
        }
        let colony = self.colonies[ci].id;
        let price = (def.keeper_coin * cfg.stall_price_scale).max(0.0);
        // An empty counter is taken over rather than duplicated.
        let vacant = (0..self.buildings.len()).find(|&bi| {
            let b = &self.buildings[bi];
            b.built && b.colony == colony && b.def.structure == Structure::Stall && b.owner == 0
        });
        if vacant.is_none() {
            // A counter needs people to sell to, and it takes its keeper out of
            // every other trade, so a town supports one per so many heads.
            let pop = self.colony_population(colony) as i32;
            let room = (pop / cfg.stall_customers.max(1)).min(cfg.stalls_per_town.max(0));
            if self.count_all(colony, def.id) as i32 >= room {
                return;
            }
            // One at a time: a half built stall is nobody's business yet.
            if self.buildings.iter().any(|b| {
                !b.built && b.colony == colony && b.def.structure == Structure::Stall
            }) {
                return;
            }
        }

        // Whoever has the coin and the temperament. Trade is a sociable
        // business, and a thrifty settler would rather keep the coin than
        // spend it on a counter.
        let mut best: Option<(usize, f64)> = None;
        for pi in self.people.live_indices() {
            let p = &self.people[pi];
            if p.colony != colony || !p.adult() || p.stall != 0 || p.coin < price {
                continue;
            }
            let score = p.traits.sociability * 1.4 + p.skill_in(Profession::Shopkeeper)
                - p.traits.thrift * 0.5
                + p.coin / 400.0;
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((pi, score));
            }
        }
        let pi = match best {
            Some((pi, _)) => pi,
            None => return,
        };
        let day = self.day;
        let person_id = self.people[pi].id;
        let name = self.people[pi].name.clone();

        if let Some(bi) = vacant {
            let id = self.buildings[bi].id;
            self.buildings[bi].owner = person_id;
            self.people[pi].stall = id;
            self.people[pi].log(day, "took over an empty stall".to_string());
            self.colonies[ci]
                .econ
                .log_event(format!("{name} took over a stall"), day);
            self.assign_workplaces(state, ci);
            return;
        }

        let anchor = self.market_cell(colony);
        let site = match find_site_near(self, state, ci, def, anchor.0, anchor.1, 8) {
            Some(s) => s,
            None => return,
        };
        let paid = self.people[pi].spend(price);
        self.colonies[ci].econ.coin += paid;
        let bi = match self.place_building(state, ci, "stall", site.0, site.1, false) {
            Some(bi) => bi,
            None => return,
        };
        let id = self.buildings[bi].id;
        self.buildings[bi].owner = person_id;
        self.people[pi].stall = id;
        self.people[pi].log(day, "paid for a stall of their own".to_string());
        self.colonies[ci]
            .econ
            .log_event(format!("{name} is opening a stall"), day);
    }

    /// The counter this settler would rather buy from: near, stocked, and kept
    /// by somebody they have taken to. `want` narrows it to one ware, which is
    /// what a hungry settler asks for.
    pub fn stall_near(&self, pi: usize, want: Option<Res>) -> Option<usize> {
        let colony = self.people[pi].colony;
        let person_id = self.people[pi].id;
        let (px, py) = (self.people[pi].x, self.people[pi].y);
        let mut best = None;
        let mut best_score = f64::NEG_INFINITY;
        for (i, b) in self.buildings.iter().enumerate() {
            if !b.built || b.colony != colony || b.def.structure != Structure::Stall {
                continue;
            }
            // Nobody buys from an empty counter, and nobody buys from themself.
            if b.owner == 0 || b.owner == person_id {
                continue;
            }
            let stocked = match want {
                Some(res) => b.inv[res as usize] >= 1.0,
                None => b.def.sells.iter().any(|&res| b.inv[res as usize] >= 1.0),
            };
            if !stocked {
                continue;
            }
            let d = (b.col as f64 - px).hypot(b.row as f64 - py);
            if d > b.def.radius.max(6.0) {
                continue;
            }
            let score = self.people[pi].affinity_for(b.owner) * 4.0 - d;
            if score > best_score {
                best_score = score;
                best = Some(i);
            }
        }
        best
    }

    /// What a keeper asks over the counter: their town's price for the ware,
    /// plus what they can get away with. A practised trader gets away with
    /// more.
    pub fn stall_price(&self, state: &State, bi: usize, res: Res) -> f64 {
        let ci = match self.colony_index(self.buildings[bi].colony) {
            Some(ci) => ci,
            None => return res.def().base_price,
        };
        let base = self.colonies[ci].econ.price_of(res);
        let skill = self
            .people
            .get(self.buildings[bi].owner)
            .map(|p| p.skill_in(Profession::Shopkeeper))
            .unwrap_or(1.0);
        base * (1.0 + state.civ.build.stall_margin.max(0.0) * skill)
    }

    // ---- inns ------------------------------------------------------------

    pub fn inn_near(&self, colony: i32, col: i32, row: i32) -> Option<usize> {
        let mut best = None;
        let mut best_d = f64::INFINITY;
        for (i, b) in self.buildings.iter().enumerate() {
            if !b.built || !b.def.is_inn || b.colony != colony {
                continue;
            }
            if b.guests.len() as i32 >= b.def.rooms {
                continue;
            }
            let d = ((b.col - col) as f64).hypot((b.row - row) as f64);
            if d < best_d {
                best_d = d;
                best = Some(i);
            }
        }
        best
    }

    fn clear_inn_guests(&mut self) {
        for b in &mut self.buildings {
            if b.def.is_inn {
                b.guests.clear();
            }
        }
    }

    // ---- piles -----------------------------------------------------------

    /// Leaves a load on the ground and says which pile it went into, so a
    /// caller with something to say about that pile can find it again.
    pub fn add_pile(&mut self, col: i32, row: i32, res: Res, n: f64) -> Option<usize> {
        if n <= 0.0 {
            return None;
        }
        // The spot is resolved before the merge, or a load dropped on a blocked
        // cell would start a new pile on every single delivery.
        let spot = self.free_cell_near(col, row).unwrap_or((col, row));
        if let Some(existing) = self
            .piles
            .iter_mut()
            .position(|q| q.col == spot.0 && q.row == spot.1 && q.res == res)
        {
            self.piles[existing].n += n;
            self.buffer_dirty = true;
            return Some(existing);
        }
        let id = self.next_pile_id;
        self.next_pile_id += 1;
        let seed = self.rng.seed();
        self.piles.push(Pile {
            id,
            col: spot.0,
            row: spot.1,
            res,
            n,
            claimed_by: 0,
            seed,
            by_hand: false,
        });
        self.buffer_dirty = true;
        Some(self.piles.len() - 1)
    }

    pub fn take_pile(&mut self, index: usize, n: f64) -> f64 {
        let got = self.piles[index].n.min(n);
        self.piles[index].n -= got;
        if self.piles[index].n <= 0.01 {
            self.piles.remove(index);
        }
        self.buffer_dirty = true;
        got
    }

    fn piles_tick(&mut self, state: &State, dt: f64) {
        let life = state.civ.work.pile_life.max(0.2) * state.civ.people.day_length;
        let keep = (-dt / life).exp();
        for i in (0..self.piles.len()).rev() {
            self.piles[i].n *= keep;
            if self.piles[i].n < 0.5 {
                self.piles.remove(i);
            }
        }
    }

    // ---- stock -----------------------------------------------------------

    /// Nobody carries home what their colony is already drowning in.
    pub fn wanted(&self, state: &State, ci: usize, res: Res) -> bool {
        let colony = match self.colonies.get(ci) {
            Some(c) => c,
            None => return false,
        };
        let pop = self.colony_population(colony.id);
        let targets = stock_targets(&state.civ.economy, pop);
        let limit = targets[res as usize] * state.civ.economy.hoard_limit;
        colony.stock[res as usize] < limit
    }

    /// Anything the store has no room for is left outside it, where it slowly
    /// rots. A colony that keeps gathering past its storage loses the surplus,
    /// which is what makes another storehouse worth building.
    pub fn deposit(&mut self, state: &State, ci: usize, res: Res, n: f64, at: Option<(i32, i32)>) -> f64 {
        if n <= 0.0 || ci >= self.colonies.len() {
            return 0.0;
        }
        let spot = at.or(Some(self.colonies[ci].center)).unwrap_or((0, 0));
        if !self.wanted(state, ci, res) {
            self.add_pile(spot.0, spot.1, res, n);
            return 0.0;
        }
        let space = self.store_space(ci);
        let bulk = res.def().bulk;
        let room = (space / bulk).floor();
        let put = n.min(room).max(0.0);
        if put > 0.0 {
            add_stock(&mut self.colonies[ci].stock, res, put);
        }
        let over = n - put;
        if over > 0.0 {
            self.add_pile(spot.0, spot.1, res, over);
        }
        put
    }

    pub fn available_stock(&self, ci: usize, res: Res) -> f64 {
        self.colonies.get(ci).map(|c| c.available(res)).unwrap_or(0.0)
    }

    pub fn reserve_stock(&mut self, ci: usize, res: Res, n: f64) {
        if let Some(c) = self.colonies.get_mut(ci) {
            c.reserve(res, n);
        }
    }

    pub fn release_stock(&mut self, ci: usize, res: Res, n: f64) {
        if let Some(c) = self.colonies.get_mut(ci) {
            c.release(res, n);
        }
    }

    /// Everything every colony holds, for the world level readouts.
    pub fn total_stock(&self) -> Stock {
        let mut out = make_stock(0.0);
        for c in &self.colonies {
            for res in RES_IDS {
                out[res as usize] += c.stock[res as usize];
            }
        }
        out
    }

    // ---- assignment ------------------------------------------------------

    /// Labor is reallocated from scratch every day: workplaces are ranked by
    /// what the colony is short of and filled from its adults, with a strong
    /// preference for keeping people where they already work. Without the
    /// reshuffle a colony staffs its quarries while it starves, because a
    /// worker already in a job is never reconsidered.
    pub fn assign_workplaces(&mut self, state: &State, ci: usize) {
        let colony = self.colonies[ci].id;
        let pcfg = &state.civ.people;
        let adults: Vec<usize> = self
            .people
            .live_indices()
            .into_iter()
            .filter(|&i| {
                self.people[i].colony == colony
                    && self.people[i].adult()
                    && self.people[i].aboard == 0
            })
            .collect();
        let reserve = ((adults.len() as f64 * clamp01(pcfg.laborer_share)).round() as usize).max(1);
        let employable = adults.len().saturating_sub(reserve);
        let previous: HashMap<u32, i32> =
            adults.iter().map(|&i| (self.people[i].id, self.people[i].work)).collect();
        for b in &mut self.buildings {
            if b.colony == colony {
                b.workers.clear();
            }
        }
        for &i in &adults {
            self.people[i].work = 0;
            self.people[i].profession = Profession::Laborer;
        }

        let mut openings: Vec<(usize, f64)> = (0..self.buildings.len())
            .filter(|&i| {
                self.buildings[i].built
                    && self.buildings[i].colony == colony
                    && self.buildings[i].def.slots > 0
                    && self.buildings[i].def.structure != Structure::Stall
            })
            .map(|i| (i, self.job_priority(state, ci, i)))
            .collect();
        openings.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut free = adults.clone();
        let mut employed = 0usize;

        // A stall is worked by whoever bought it: the colony neither staffs it
        // nor may take it away. Keepers are pinned before anything else is
        // filled, so a shopkeeper is never marched off to a quarry on the day
        // the store runs short of stone.
        for bi in 0..self.buildings.len() {
            let b = &self.buildings[bi];
            if !b.built || b.colony != colony || b.def.structure != Structure::Stall {
                continue;
            }
            let owner = b.owner;
            let pi = match self.people.index_of(owner) {
                Some(pi) if self.people[pi].alive && self.people[pi].colony == colony => pi,
                _ => continue,
            };
            let k = match free.iter().position(|&f| f == pi) {
                Some(k) => k,
                None => continue,
            };
            free.remove(k);
            let bid = self.buildings[bi].id;
            self.people[pi].work = bid;
            self.people[pi].profession = Profession::Shopkeeper;
            self.buildings[bi].workers.push(owner);
            employed += 1;
        }

        for (bi, priority) in openings {
            if priority <= -0.6 {
                continue;
            }
            let slots = self.buildings[bi].def.slots;
            while self.buildings[bi].workers.len() < slots && employed < employable && !free.is_empty()
            {
                let (bcol, brow) = (self.buildings[bi].col as f64, self.buildings[bi].row as f64);
                let bid = self.buildings[bi].id;
                let prof = profession_for(self.buildings[bi].def);
                let mut best_at = 0;
                let mut best_score = f64::NEG_INFINITY;
                for (k, &pi) in free.iter().enumerate() {
                    let p = &self.people[pi];
                    let dist = (p.x - bcol).hypot(p.y - brow);
                    let sticky = if previous.get(&p.id) == Some(&bid) { 12.0 } else { 0.0 };
                    // A trade somebody is already good at is worth a walk.
                    let skilled = (p.skill_in(prof) - 1.0) * 6.0;
                    let score = sticky + skilled - dist * 0.2;
                    if score > best_score {
                        best_score = score;
                        best_at = k;
                    }
                }
                let pi = free.remove(best_at);
                self.people[pi].work = bid;
                self.people[pi].profession = prof;
                let person_id = self.people[pi].id;
                self.buildings[bi].workers.push(person_id);
                employed += 1;
            }
        }

        let mut abandon = Vec::new();
        for pi in self.people.live_indices() {
            if self.people[pi].colony != colony {
                continue;
            }
            if !self.people[pi].adult() {
                self.people[pi].profession = Profession::Child;
                continue;
            }
            if self.people[pi].aboard != 0 {
                self.people[pi].profession = Profession::Sailor;
                continue;
            }
            let p = &self.people[pi];
            let moved = previous.get(&p.id).is_some_and(|&w| w != p.work);
            if moved && p.task.as_ref().is_some_and(|t| !t.is_sleep()) {
                abandon.push(pi);
            }
        }
        for pi in abandon {
            abandon_task(self, pi);
        }
    }

    /// How badly an open workplace wants filling, given what its colony's store
    /// is short of. Food first, then whatever the build planner is waiting on.
    pub fn job_priority(&self, state: &State, ci: usize, bi: usize) -> f64 {
        let colony = self.colonies[ci].id;
        let pop = self.colony_population(colony);
        let targets = stock_targets(&state.civ.economy, pop);
        let stock = &self.colonies[ci].stock;
        let b = &self.buildings[bi];
        let job = match &b.def.job {
            Some(j) => j,
            None => return 0.0,
        };
        match job {
            Job::Harvest { classes, yields, .. } => {
                // A camp with nothing left to cut within reach is not worth
                // staffing, however badly the store wants what it used to
                // produce. This is what pushes a colony off foraging and onto
                // farming.
                let radius = if b.def.radius > 0.0 { b.def.radius } else { 12.0 } * 1.5;
                let mass = self.harvestable_mass(state, b.col, b.row, radius, classes);
                let supply = clamp01(mass / (b.def.slots.max(1) as f64 * 40.0));
                self.gather_priority(state, ci, &targets, yields, supply, 0.0)
            }
            Job::Mine { deposit, yields } => {
                let radius = if b.def.radius > 0.0 { b.def.radius } else { 12.0 };
                let supply = if self.terrain.find_deposit(*deposit, b.col, b.row, radius).is_some() {
                    1.0
                } else {
                    0.0
                };
                self.gather_priority(state, ci, &targets, yields, supply, 0.0)
            }
            // Farming is the reliable half of the food supply, so it outranks a
            // forager camp when both are hungry for hands.
            Job::Farm { yields } => self.gather_priority(state, ci, &targets, yields, 1.0, 0.3),
            Job::Craft { input, output, .. } => {
                let mut inputs: f64 = 1.0;
                for &(res, n) in input.iter() {
                    inputs = inputs.min(stock[res as usize] / (n * 4.0).max(1.0));
                }
                let mut want: f64 = -1.0;
                for &(res, _) in output.iter() {
                    let short = clamp(
                        (targets[res as usize] - stock[res as usize]) / targets[res as usize].max(1.0),
                        -1.0,
                        1.0,
                    );
                    want = want.max(short * self.demand_for(ci, res));
                }
                want * inputs - 0.2
            }
            Job::Research => 0.35,
            Job::Trade => 0.3,
            // Somebody has to be behind the bar for the rooms upstairs to
            // count, and a town with people sleeping rough wants that badly.
            Job::Innkeep => 0.25 + clamp01(self.roofless(colony) as f64 / 4.0) * 0.9,
            Job::Ferry => {
                if self.colonies.len() > 1 {
                    0.55
                } else {
                    -0.2
                }
            }
            // Never: a stall is worked by whoever bought it, and by nobody
            // else. It is filled before this list is even looked at.
            Job::Sell => -1.0,
        }
    }

    /// The scarcest thing a gathering job produces sets its priority. Summing
    /// instead lets a byproduct nobody needs cancel out a shortage of the main
    /// one, which is how a colony ends up with no wood and eight foragers.
    fn gather_priority(
        &self,
        _state: &State,
        ci: usize,
        targets: &Stock,
        yields: &[(Res, f64)],
        supply: f64,
        settled: f64,
    ) -> f64 {
        let stock = &self.colonies[ci].stock;
        let mut need: f64 = -1.0;
        let mut has_food = false;
        for &(res, _) in yields.iter() {
            if res == Res::Food {
                has_food = true;
            }
            let short = clamp(
                (targets[res as usize] - stock[res as usize]) / targets[res as usize].max(1.0),
                -1.0,
                1.0,
            );
            need = need.max(short * self.demand_for(ci, res));
        }
        if has_food {
            (0.3 * supply).max((need + 0.6 + settled) * supply)
        } else {
            need * supply
        }
    }

    /// Standing biomass a camp could still cut, read out of the plant index
    /// rather than off every plant on the map.
    pub fn harvestable_mass(
        &self,
        state: &State,
        col: i32,
        row: i32,
        radius: f64,
        classes: &[SizeClass],
    ) -> f64 {
        let min = state.civ.work.min_harvest_mass as f32;
        let r2 = radius * radius;
        let mut total = 0.0;
        self.plant_index.near(col, row, radius, |mark| {
            if mark.mass < min || !classes.contains(&mark.class) {
                return;
            }
            let dx = (mark.col - col) as f64;
            let dy = (mark.row - row) as f64;
            if dx * dx + dy * dy > r2 {
                return;
            }
            total += mark.mass as f64;
        });
        total
    }

    /// How much a colony actually wants a resource, beyond the abstract stock
    /// target: something nothing consumes and nothing is built out of is barely
    /// worth making. Charcoal only matters once a kiln stands.
    pub fn demand_for(&self, ci: usize, res: Res) -> f64 {
        if res == Res::Food || res == Res::Wood {
            return 1.0;
        }
        let colony = self.colonies[ci].id;
        let mut demand: f64 = 0.15;
        for b in &self.buildings {
            if !b.built || b.colony != colony {
                continue;
            }
            if let Some(Job::Craft { input, .. }) = &b.def.job {
                if input.iter().any(|&(r, _)| r == res) {
                    return 1.0;
                }
            }
        }
        for def in BUILDINGS {
            if !def.base && !self.colonies[ci].unlocked.contains(def.id) {
                continue;
            }
            if def.cost.iter().any(|&(r, _)| r == res) {
                demand = demand.max(0.85);
            }
            // A recipe that is unlocked but not yet standing still counts, or
            // the colony would never make the charcoal it needs to build the
            // kiln that would have created the demand for charcoal.
            if let Some(Job::Craft { input, .. }) = &def.job {
                if input.iter().any(|&(r, _)| r == res) {
                    demand = demand.max(0.7);
                }
            }
        }
        demand
    }

    pub fn roofless(&self, colony: i32) -> usize {
        self.colony_index(colony).map(|i| self.colonies[i].roofless).unwrap_or(0)
    }

    // ---- main step -------------------------------------------------------

    /// How long the settlement has been empty, in its own time. None while
    /// anybody is alive, so a paused world never counts down.
    pub fn extinct_for(&self) -> Option<f64> {
        self.extinct_at.map(|at| self.time - at)
    }

    pub fn step(&mut self, state: &State, dt: f64) {
        if !self.ready {
            return;
        }
        let cfg = &state.civ;
        self.swim_cost = cfg.people.swim_cost as f32;
        self.block_mass = block_mass(state);
        self.plant_sim.fall_time = cfg.work.fall_time.max(0.05);
        self.time += dt;
        self.ticks += 1;
        {
            let _t = phases::time(Phase::Refresh);
            self.refresh_colonies();
        }
        let blocked = std::mem::take(&mut self.blocked);
        {
            let _t = phases::time(Phase::Plants);
            self.plant_sim.step(state, dt, Some(&blocked));
        }
        self.blocked = blocked;
        // A plant that was re-drawn or removed changes the shadows on the
        // ground.
        if self.plant_sim.buffer_dirty {
            self.plant_sim.buffer_dirty = false;
            self.ground_dirty = true;
        }

        self.plant_index.timer -= dt;
        if self.plant_index.timer <= 0.0 {
            self.plant_index.timer = cfg.work.plant_index_interval.max(0.1);
            let _t = phases::time(Phase::PlantIndex);
            self.rebuild_plant_index();
        }

        {
            let _t = phases::time(Phase::Plan);
            for ci in 0..self.colonies.len() {
                self.colonies[ci].plan_timer -= dt;
                if self.colonies[ci].plan_timer <= 0.0 {
                    self.colonies[ci].plan_timer = cfg.work.plan_interval.max(0.1);
                    if self.is_live(ci) {
                        plan(self, state, ci);
                        plan_walls(self, state, ci);
                    }
                }
            }
        }

        {
            let _t = phases::time(Phase::People);
            for pi in self.people.live_indices() {
                update_person(self, state, pi, dt);
                if !self.people[pi].alive {
                    self.bury_person(state, pi);
                }
            }
        }

        // Whether there is anybody left, noted the moment it changes so the
        // restart counts from the death rather than from being noticed.
        let anybody = !self.people.live_indices().is_empty();
        match (anybody, self.extinct_at) {
            (false, None) => self.extinct_at = Some(self.time),
            (true, Some(_)) => self.extinct_at = None,
            _ => {}
        }

        // Damp ground fills a farm back up on its own. Every farm, every
        // tick: there are a handful of them and the soak is a square search
        // over their own fields.
        {
            let _t = phases::time(Phase::Farms);
            let reach = cfg.work.farm_soak_reach.max(0);
            for bi in 0..self.buildings.len() {
                if !matches!(self.buildings[bi].def.job, Some(Job::Farm { .. })) {
                    continue;
                }
                if !self.buildings[bi].built {
                    continue;
                }
                let soak = match self.buildings[bi].soak {
                    Some((r, s)) if r == reach => s,
                    _ => {
                        let s = self.farm_soak(state, bi);
                        self.buildings[bi].soak = Some((reach, s));
                        s
                    }
                };
                if soak <= 0.0 {
                    continue;
                }
                let gain = soak * cfg.work.farm_soak_rate * dt;
                self.buildings[bi].water = clamp01(self.buildings[bi].water + gain);
            }
        }

        // After everyone has moved, because who is standing next to whom is
        // the whole input.
        {
            let _t = phases::time(Phase::Social);
            social_tick(self, state, dt);
        }
        {
            let _t = phases::time(Phase::Production);
            self.production_tick();
            self.piles_tick(state, dt);
        }
        {
            let _t = phases::time(Phase::Economy);
            for ci in 0..self.colonies.len() {
                self.economy_tick(state, ci, dt);
                self.research_tick(state, ci, dt);
            }
        }
        {
            let _t = phases::time(Phase::Boats);
            boats_tick(self, state, dt);
            balloons_tick(self, state, dt);
        }

        let day = day_number(self.time, &cfg.people);
        if day != self.day {
            self.day = day;
            let _t = phases::time(Phase::Day);
            self.day_tick(state);
        }

        // Footpaths fade unless they keep being walked. The whole grid is swept
        // at most once a simulated second, with the elapsed time compounded
        // into the decay, rather than every tick.
        self.traffic_timer += dt;
        if self.traffic_timer >= 1.0 {
            let _t = phases::time(Phase::Traffic);
            let decay = (-self.traffic_timer * 0.02).exp() as f32;
            self.traffic_timer = 0.0;
            for v in &mut self.traffic {
                if *v > 0.002 {
                    *v *= decay;
                } else {
                    *v = 0.0;
                }
            }
        }
        self.buffer_dirty = true;
    }

    fn day_tick(&mut self, state: &State) {
        self.clear_inn_guests();
        build_boats(self, state);
        let day = self.day;
        for ci in 0..self.colonies.len() {
            if self.colonies[ci].abandoned {
                // Said once, on the day it happens, rather than every day after.
                if self.colonies[ci].emptied_day.is_none() {
                    self.colonies[ci].emptied_day = Some(day);
                    let name = self.colonies[ci].name.clone();
                    self.colonies[ci]
                        .econ
                        .log_event(format!("{name} stands empty"), day);
                }
                continue;
            }
            self.colonies[ci].econ.roll_flows();
            self.assign_workplaces(state, ci);
            self.match_couples(state, ci);
            self.population_tick(state, ci);
            self.upgrade_homes(state, ci);
            self.light_by_fear(state, ci);
            self.open_stalls(state, ci);
            self.decay_food(state, ci);
            self.expedition_tick(state, ci);

            let colony = self.colonies[ci].id;
            let sample = Sample {
                day: self.day,
                pop: self.colony_population(colony) as f64,
                coin: self.colonies[ci].econ.coin.round(),
                food: self.colonies[ci].stock[Res::Food as usize].round(),
                wood: self.colonies[ci].stock[Res::Wood as usize].round(),
                research: self.colonies[ci].tech.points.round(),
                buildings: self.buildings.iter().filter(|b| b.built && b.colony == colony).count()
                    as f64,
                happiness: self.colony_happiness(colony),
            };
            self.colonies[ci].econ.push_history(&state.civ.economy, sample);
        }
        self.crumble_tick(state);
        // The archive is only trimmed here, where nothing is holding a slot.
        self.people.prune(state.civ.people_archive.max(50));
        self.refresh_colonies();
    }

    /// A house nobody lives in comes down, a day at a time.
    ///
    /// Not the moment it empties: a home between one owner and the next is
    /// somewhere the town is about to put somebody, and pulling it down would
    /// be the opposite of what a town does with a spare bed. Past the wait it
    /// starts to go, and somebody moving in at any point puts it right again
    /// at the same rate it was going wrong. Every town's houses are looked at,
    /// including a town with nobody left in it: an abandoned settlement
    /// falling in is the whole reason for this.
    fn crumble_tick(&mut self, state: &State) {
        let cfg = &state.civ.build;
        if !cfg.crumble {
            return;
        }
        let wait = cfg.crumble_after.max(0.0);
        let over = cfg.crumble_days.max(0.5);
        let salvage = clamp01(cfg.crumble_salvage);
        let mut fallen: Vec<i32> = Vec::new();
        for bi in 0..self.buildings.len() {
            let b = &self.buildings[bi];
            if b.def.housing <= 0 || !b.built || b.upgrading {
                continue;
            }
            // Somebody living here, rather than somebody on the deed: a house
            // whose household has died is empty however the register reads.
            let lived_in = b.residents.iter().any(|&id| self.people.is_alive(id));
            let b = &mut self.buildings[bi];
            if lived_in {
                b.empty_days = 0.0;
                b.decay = (b.decay - 1.0 / over).max(0.0);
            } else {
                b.empty_days += 1.0;
                if b.empty_days > wait {
                    b.decay += 1.0 / over;
                }
            }
            if b.decay >= 1.0 {
                fallen.push(b.id);
            }
        }
        for id in fallen {
            let bi = match self.building_index(id) {
                Some(bi) => bi,
                None => continue,
            };
            let at = self.access_cell(bi);
            let rubble: Vec<(Res, f64)> = self.buildings[bi]
                .cost
                .iter()
                .map(|&(res, n)| (res, (n * salvage).round()))
                .filter(|&(_, n)| n >= 1.0)
                .collect();
            let (label, colony) = (self.buildings[bi].label(), self.buildings[bi].colony);
            self.remove_building(bi);
            // What it was built from is left where it stood, so a town that
            // has one fall in gets some of its timber back if anybody is left
            // to fetch it.
            for (res, n) in rubble {
                self.add_pile(at.0, at.1, res, n);
            }
            if let Some(ci) = self.colony_index(colony) {
                let day = self.day;
                self.colonies[ci].econ.log_event(format!("{label} fell in"), day);
            }
            self.ground_dirty = true;
        }
    }

    fn decay_food(&mut self, state: &State, ci: usize) {
        let colony = self.colonies[ci].id;
        let keep = if self
            .buildings
            .iter()
            .any(|b| b.built && b.def.keeps_food && b.colony == colony)
        {
            0.35
        } else {
            1.0
        };
        for res in RES_IDS {
            let rate = res.def().decay * keep;
            if rate == 0.0 {
                continue;
            }
            let lost = self.colonies[ci].stock[res as usize] * rate * state.civ.people.day_length;
            if lost >= 0.5 {
                let n = lost.floor();
                take_stock(&mut self.colonies[ci].stock, res, n);
                self.colonies[ci].econ.record_consumed(res, n);
            }
        }
    }

    // ---- people ----------------------------------------------------------

    fn bury_person(&mut self, state: &State, index: usize) {
        // The register decides whether this is a burial or a slot that was
        // already buried; the alive flag is only the task system's opinion.
        if !self.people.retire(index) {
            return;
        }
        let ci = self.colony_of(index);
        let carry = self.people[index].carry;
        if carry.n > 0.0 {
            if let Some(res) = carry.res {
                self.deposit(state, ci, res, carry.n, None);
            }
        }
        abandon_task(self, index);
        self.leave_building(index);
        if self.people[index].aboard != 0 {
            let boat = self.people[index].aboard;
            if let Some(b) = self.boats.iter_mut().find(|b| b.id == boat) {
                let id = self.people[index].id;
                b.crew.retain(|&c| c != id);
            }
            self.people[index].aboard = 0;
        }
        self.refresh_colonies();

        let (name, age, cause, id, home, spouse) = {
            let p = &self.people[index];
            (
                p.name.clone(),
                p.age.floor() as i32,
                p.cause.clone().unwrap_or_else(|| "old age".to_string()),
                p.id,
                p.home,
                p.spouse,
            )
        };
        self.people[index].died = self.day;
        self.deaths += 1;
        if let Some(c) = self.colonies.get_mut(ci) {
            c.deaths += 1;
        }
        let colony = self.people[index].colony;
        self.dead.push(Obituary {
            name: name.clone(),
            age,
            cause: cause.clone(),
            day: self.day,
            colony,
        });
        if self.dead.len() > 60 {
            self.dead.remove(0);
        }
        if spouse != 0 {
            if let Some(si) = self.people.index_of(spouse) {
                if self.people[si].alive {
                    self.people[si].spouse = 0;
                    let day = self.day;
                    self.people[si].log(day, format!("was widowed by {name}"));
                }
            }
        }
        // The deed passes to whoever else is under that roof, oldest first.
        // A stall does not: it stands there with nobody behind the counter
        // until somebody else takes it on.
        self.release_stall(index);
        self.inherit_home(index);
        let day = self.day;
        self.colonies[ci]
            .econ
            .log_event(format!("{name} died of {cause} at {age}"), day);
        if home != 0 {
            self.assign_homes(ci);
        }
        let _ = id;
        self.assign_workplaces(state, ci);
    }

    fn inherit_home(&mut self, index: usize) {
        let owns = self.people[index].owns;
        if owns == 0 {
            return;
        }
        self.release_deed(index);
        let bi = match self.building_index(owns) {
            Some(bi) => bi,
            None => return,
        };
        let heirs = self.buildings[bi].residents.clone();
        let mut best: Option<(usize, f64)> = None;
        for id in heirs {
            let pi = match self.people.index_of(id) {
                Some(pi) => pi,
                None => continue,
            };
            if !self.people[pi].alive || !self.people[pi].adult() {
                continue;
            }
            // An heir with a roof of their own already has one deed.
            if self.people[pi].owns != 0 {
                continue;
            }
            let age = self.people[pi].age;
            if best.is_none_or(|(_, a)| age > a) {
                best = Some((pi, age));
            }
        }
        if let Some((pi, _)) = best {
            let person_id = self.people[pi].id;
            self.buildings[bi].owner = person_id;
            self.people[pi].owns = owns;
            let day = self.day;
            let what = self.buildings[bi].def.label.to_lowercase();
            self.people[pi].log(day, format!("inherited the {what}"));
        }
    }

    /// Marriages, once a day. Two unattached adults of a colony who are not
    /// close kin pair off; a sociable settler does it sooner.
    fn match_couples(&mut self, state: &State, ci: usize) {
        let colony = self.colonies[ci].id;
        let cfg = &state.civ.people;
        let mut singles: Vec<usize> = self
            .people
            .live_indices()
            .into_iter()
            .filter(|&pi| {
                let p = &self.people[pi];
                p.colony == colony
                    && p.spouse == 0
                    && p.age >= cfg.marry_age
                    && p.age < cfg.fertile_until + 12.0
            })
            .collect();
        if singles.len() < 2 {
            return;
        }
        singles.sort_by(|&a, &b| {
            self.people[b]
                .traits
                .sociability
                .partial_cmp(&self.people[a].traits.sociability)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut taken: HashSet<usize> = HashSet::new();
        for k in 0..singles.len() {
            let a = singles[k];
            if taken.contains(&a) || self.people[a].spouse != 0 {
                continue;
            }
            if !self.rng.chance(0.15 + self.people[a].traits.sociability * 0.35) {
                continue;
            }
            let scfg = state.civ.social;
            let mut best: Option<(usize, f64)> = None;
            for &b in singles.iter().skip(k + 1) {
                if taken.contains(&b) || self.people[b].spouse != 0 || self.related(a, b) {
                    continue;
                }
                let bid = self.people[b].id;
                let fond = self.people[a].affinity_for(bid);
                let dx = self.people[a].x - self.people[b].x;
                let dy = self.people[a].y - self.people[b].y;
                let d = dx.hypot(dy);
                let gap = (self.people[a].age - self.people[b].age).abs();
                // Age and distance decide who is a plausible match at all;
                // affinity decides between the plausible ones, and its say
                // falls away as the ages diverge.
                //
                // Weighing affinity against the age gap instead is what a
                // first attempt did, and it quietly emptied towns: a strong
                // friendship would marry somebody a generation older, that
                // couple would be past bearing within a few years, and a
                // settlement that ages fast enough stops having children
                // altogether. Nothing in the bookkeeping notices.
                let score = -gap * 0.4 - d * 0.1 + fond * scfg.courtship / (1.0 + gap);
                if best.is_none_or(|(_, s)| score > s) {
                    best = Some((b, score));
                }
            }
            let b = match best {
                Some((b, _)) => b,
                None => continue,
            };
            taken.insert(a);
            taken.insert(b);
            let (aid, bid) = (self.people[a].id, self.people[b].id);
            let (aname, bname) = (self.people[a].name.clone(), self.people[b].name.clone());
            self.people[a].spouse = bid;
            self.people[b].spouse = aid;
            self.bind_kin(a, b, state.civ.social.memory);
            let day = self.day;
            self.people[a].log(day, format!("married {bname}"));
            self.people[b].log(day, format!("married {aname}"));
            self.colonies[ci]
                .econ
                .log_event(format!("{aname} and {bname} married"), day);
        }
        if !taken.is_empty() {
            self.assign_homes(ci);
        }
    }

    /// Family, for the purpose of a bond: married, or related by blood. A kin
    /// bond is never forgotten to make room for a stranger, which is why the
    /// question is asked at every encounter rather than only at a wedding.
    pub fn kin(&self, a: usize, b: usize) -> bool {
        let (pa, pb) = (&self.people[a], &self.people[b]);
        if pa.spouse != 0 && pa.spouse == pb.id {
            return true;
        }
        self.related(a, b)
    }

    /// Files a family bond on both sides at once.
    fn bind_kin(&mut self, a: usize, b: usize, cap: usize) {
        if a == b {
            return;
        }
        let day = self.day;
        let (aid, bid) = (self.people[a].id, self.people[b].id);
        self.people[a].bind_kin(bid, day, cap);
        self.people[b].bind_kin(aid, day, cap);
    }

    /// Parent, child or sibling. Enough to keep a family tree from folding in
    /// on itself without carrying a full pedigree.
    fn related(&self, a: usize, b: usize) -> bool {
        let (pa, pb) = (&self.people[a], &self.people[b]);
        if pa.mother != 0 && (pa.mother == pb.mother || pa.mother == pb.id) {
            return true;
        }
        if pa.father != 0 && (pa.father == pb.father || pa.father == pb.id) {
            return true;
        }
        pb.mother == pa.id || pb.father == pa.id
    }

    /// Married pairs who could have a child. Counted once per pair and tested
    /// as a pair: one partner staying at an inn while their house is rebuilt
    /// does not make the household infertile, and it is the younger of the two
    /// whose age decides.
    fn fertile_couples(&self, pcfg: &crate::civ::people::PeopleConfig, colony: i32) -> Vec<usize> {
        let mut out = Vec::new();
        for pi in self.people.live_indices() {
            let p = &self.people[pi];
            if p.colony != colony || !p.adult() || p.spouse == 0 {
                continue;
            }
            let mate = match self.people.get(p.spouse) {
                Some(m) if m.alive => m,
                _ => continue,
            };
            // Once per pair, from the side with the lower id.
            if mate.id < p.id {
                continue;
            }
            if p.age.min(mate.age) >= pcfg.fertile_until {
                continue;
            }
            if p.home == 0 && mate.home == 0 {
                continue;
            }
            out.push(pi);
        }
        out
    }

    fn population_tick(&mut self, state: &State, ci: usize) {
        let pcfg = &state.civ.people;
        let colony = self.colonies[ci].id;
        let capacity = self.housing_capacity(colony) as usize;
        let pop = self.colony_population(colony);
        if pop >= capacity {
            return;
        }
        // Days of food per person in store, not the size of the heap: a growing
        // colony has to keep pace with itself to keep growing.
        let food = self.colonies[ci].stock[Res::Food as usize];
        let fed = clamp01(food / (pop as f64 * pcfg.meal_size * 3.0).max(1.0));
        let couples = self.fertile_couples(pcfg, colony);
        // Births per couple per day, thinned by how well fed and housed they are.
        let rate = pcfg.birth_rate * self.colonies[ci].mods.comfort * fed * couples.len() as f64;
        let mut births = rate.floor() as i32;
        if self.rng.chance(rate - births as f64) {
            births += 1;
        }
        if couples.is_empty() {
            return;
        }
        // Which couple: a random one, stepped through from there. Taking the
        // first every time is not a rounding detail - it means every child in
        // the town has the same mother, the whole next generation are siblings,
        // none of them can marry each other, and the town dies out one
        // generation later without ever running short of anything.
        let start = self.rng.int(0, couples.len() as i32 - 1).max(0) as usize;
        let mut born = 0;
        for k in 0..births as usize {
            if self.colony_population(colony) >= capacity {
                break;
            }
            let parent = couples[(start + k) % couples.len()];
            let spouse_id = self.people[parent].spouse;
            let (col, row, mut home) = {
                let p = &self.people[parent];
                (p.cell_col(), p.cell_row(), p.home)
            };
            if home == 0 {
                home = self.people.get(spouse_id).map(|m| m.home).unwrap_or(0);
            }
            let id = self.people.claim_id();
            let mut child = Person::new(id, col, row, 0.0, &mut self.rng);
            child.adult_age = pcfg.adult_age;
            child.lifespan = self.rng.int(pcfg.lifespan_min, pcfg.lifespan_max) as f64;
            child.home = home;
            child.born = self.day;
            child.colony = colony;
            child.born_in = colony;
            child.mother = self.people[parent].id;
            child.father = spouse_id;
            child.family = self.people[parent].family.clone();
            child.name = format!("{} {}", child.given, child.family);
            if let Some(other) = self.people.get(spouse_id) {
                child.traits = Traits::inherit(&self.people[parent].traits, &other.traits, &mut self.rng);
            }
            // Hardier parents pass on a little of it beyond the trait itself.
            child.health = 1.0;
            let name = child.name.clone();
            child.log(self.day, "was born".to_string());
            let child_i = self.people.insert(child);
            self.people[parent].children.push(id);
            if let Some(si) = self.people.index_of(spouse_id) {
                self.people[si].children.push(id);
            }
            // A child knows its household before it has met anybody: parents
            // first, then whichever brothers and sisters are still alive.
            let cap = state.civ.social.memory;
            self.bind_kin(child_i, parent, cap);
            if let Some(si) = self.people.index_of(spouse_id) {
                self.bind_kin(child_i, si, cap);
            }
            for sid in self.people[parent].children.clone() {
                if sid == id {
                    continue;
                }
                if let Some(sib) = self.people.index_of(sid) {
                    if self.people[sib].alive {
                        self.bind_kin(child_i, sib, cap);
                    }
                }
            }
            let day = self.day;
            self.people[parent].log(day, format!("had a child, {name}"));
            self.births += 1;
            self.colonies[ci].births += 1;
            born += 1;
            self.colonies[ci].econ.log_event(format!("{name} was born"), day);
        }
        if born > 0 {
            self.refresh_colonies();
            self.assign_homes(ci);
        }
    }

    /// When a colony is crowded and well stocked, a party of the restless walks
    /// away and founds another town. This is the only way a second colony ever
    /// appears.
    fn expedition_tick(&mut self, state: &State, ci: usize) {
        let cfg = &state.civ.build;
        if !cfg.expeditions || !self.is_live(ci) {
            return;
        }
        // Emptied towns do not count against the limit; their ground is free.
        let live = self.colonies.iter().filter(|c| !c.abandoned).count() as i32;
        if live >= cfg.max_colonies {
            return;
        }
        self.colonies[ci].expedition_timer -= state.civ.people.day_length;
        if self.colonies[ci].expedition_timer > 0.0 {
            return;
        }
        self.colonies[ci].expedition_timer = cfg.expedition_interval;
        let colony = self.colonies[ci].id;
        let pop = self.colony_population(colony);
        if (pop as i32) < cfg.expedition_population {
            return;
        }
        let food = self.colonies[ci].stock[Res::Food as usize];
        let wood = self.colonies[ci].stock[Res::Wood as usize];
        if food < cfg.expedition_supplies || wood < cfg.expedition_supplies {
            return;
        }
        let party = self.pick_expedition(state, ci, cfg.expedition_party);
        if (party.len() as i32) < cfg.expedition_party {
            return;
        }
        let spot = match self.pick_colony_site(state, ci) {
            Some(s) => s,
            None => return,
        };

        let new_ci = self.found_colony(state, spot, colony);
        let new_id = self.colonies[new_ci].id;
        let name = self.colonies[new_ci].name.clone();
        // The party carries its founding stores out of the parent's storehouse.
        for res in [Res::Food, Res::Wood, Res::Fiber, Res::Stone] {
            let n = take_stock(&mut self.colonies[ci].stock, res, cfg.expedition_supplies);
            add_stock(&mut self.colonies[new_ci].stock, res, n);
        }
        let purse = self.colonies[ci].econ.coin * 0.25;
        self.colonies[ci].econ.coin -= purse;
        self.colonies[new_ci].econ.coin += purse;
        // Knowledge travels with the people who have it.
        self.colonies[new_ci].tech.known = self.colonies[ci].tech.known.clone();
        self.colonies[new_ci].refresh_tech();

        let day = self.day;
        for pi in party {
            self.leave_building(pi);
            abandon_task(self, pi);
            self.release_deed(pi);
            self.release_stall(pi);
            self.people[pi].colony = new_id;
            self.people[pi].home = 0;
            self.people[pi].work = 0;
            self.people[pi].x = spot.0 as f64 + 0.5;
            self.people[pi].y = spot.1 as f64 + 0.5;
            self.people[pi].log(day, format!("left to found {name}"));
            // A household walks out together.
            let spouse = self.people[pi].spouse;
            let children = self.people[pi].children.clone();
            for id in std::iter::once(spouse).chain(children) {
                if id == 0 {
                    continue;
                }
                if let Some(ki) = self.people.index_of(id) {
                    if !self.people[ki].alive || self.people[ki].colony == new_id {
                        continue;
                    }
                    self.leave_building(ki);
                    abandon_task(self, ki);
                    self.release_deed(ki);
                    self.release_stall(ki);
                    self.people[ki].colony = new_id;
                    self.people[ki].home = 0;
                    self.people[ki].work = 0;
                    self.people[ki].x = spot.0 as f64 + 0.5;
                    self.people[ki].y = spot.1 as f64 + 0.5;
                    self.people[ki].log(day, format!("moved to {name}"));
                }
            }
        }

        if let Some(def) = building_by_id("storehouse") {
            if let Some(site) = find_site_near(self, state, new_ci, def, spot.0, spot.1, 8) {
                self.place_building(state, new_ci, "storehouse", site.0, site.1, true);
            }
        }
        self.refresh_colonies();
        let settlers = self.colony_population(new_id);
        let from = self.colonies[ci].name.clone();
        self.colonies[new_ci]
            .econ
            .log_event(format!("{settlers} settlers from {from} found {name}"), day);
        self.colonies[ci]
            .econ
            .log_event(format!("an expedition left to found {name}"), day);
        self.assign_homes(ci);
        self.assign_workplaces(state, ci);
        self.assign_workplaces(state, new_ci);
    }

    /// The restless, the unhoused and the unattached go first.
    fn pick_expedition(&self, state: &State, ci: usize, want: i32) -> Vec<usize> {
        let colony = self.colonies[ci].id;
        let mut cands: Vec<(usize, f64)> = self
            .people
            .live_indices()
            .into_iter()
            .filter(|&pi| {
                let p = &self.people[pi];
                let settled = self
                    .building_index(p.owns)
                    .is_some_and(|bi| home_rank(self.buildings[bi].def) > 0);
                p.colony == colony
                    && p.adult()
                    && p.aboard == 0
                    && p.age < state.civ.people.fertile_until
                    // A hut is not something anybody stays for; a house is.
                    && !settled
            })
            .map(|pi| {
                let p = &self.people[pi];
                let score = p.traits.wanderlust * 2.0
                    + if p.home == 0 { 1.0 } else { 0.0 }
                    - p.happiness * 0.5;
                (pi, score)
            })
            .collect();
        cands.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        cands.truncate(want.max(0) as usize);
        cands.into_iter().map(|(pi, _)| pi).collect()
    }

    /// Far enough from every existing town to be its own place, close enough
    /// to be reachable, and on ground worth settling.
    fn pick_colony_site(&mut self, state: &State, ci: usize) -> Option<(i32, i32)> {
        let cfg = &state.civ.build;
        let from = self.colonies[ci].center;
        let min_gap = cfg.colony_spacing.max(8) as f64;
        let centers: Vec<(i32, i32)> =
            self.colonies.iter().filter(|c| !c.abandoned).map(|c| c.center).collect();
        let mut best: Option<((i32, i32), f64)> = None;
        for _ in 0..500 {
            let c = self.rng.int(3, self.world().cols - 4);
            let r = self.rng.int(3, self.world().rows - 4);
            if !self.terrain.is_buildable(c, r) || self.terrain.deposit_at(c, r).is_some() {
                continue;
            }
            if !self.walkable(c, r) {
                continue;
            }
            let gap = centers
                .iter()
                .map(|&(x, y)| ((x - c) as f64).hypot((y - r) as f64))
                .fold(f64::INFINITY, f64::min);
            if gap < min_gap {
                continue;
            }
            let mut score = 0.0;
            let mut open = 0.0;
            for y in r - 3..=r + 3 {
                for x in c - 3..=c + 3 {
                    if self.terrain.is_buildable(x, y) && self.terrain.deposit_at(x, y).is_none() {
                        open += 1.0;
                    }
                    score += self.terrain.fertility(x, y) * 0.4;
                }
            }
            if open < 30.0 {
                continue;
            }
            score += open * 0.08;
            // Water on the doorstep is worth a great deal: it is what lets the
            // two towns trade by boat instead of by foot.
            if self.terrain.near_water(c, r, 5) {
                score += 4.0;
            }
            if self.terrain.is_river(c, r) {
                score += 1.0;
            }
            // Timber to build with.
            score += clamp(
                self.harvestable_mass(state, c, r, 16.0, &[SizeClass::Tree, SizeClass::Shrub]) * 0.02,
                0.0,
                6.0,
            );
            let reach = ((from.0 - c) as f64).hypot((from.1 - r) as f64);
            score -= (reach - min_gap * 1.6).max(0.0) * 0.05;
            if best.is_none_or(|(_, s)| score > s) {
                best = Some(((c, r), score));
            }
        }
        let (spot, _) = best?;
        // A site nobody can walk to is not a site.
        self.find_path(from.0, from.1, spot.0, spot.1)?;
        Some(spot)
    }

    /// Keeps the well and lamp lists in step with the buildings. Called
    /// wherever a building is raised, felled or swapped for another rung, so
    /// the two lookups below never see yesterday's town.
    pub fn refresh_sources(&mut self) {
        self.health_sources.clear();
        self.light_sources.clear();
        for b in &self.buildings {
            if !b.built {
                continue;
            }
            if b.def.health != 0.0 {
                let radius = if b.def.radius > 0.0 { b.def.radius } else { 10.0 };
                self.health_sources.push((
                    b.col as f64,
                    b.row as f64,
                    radius,
                    b.def.health,
                    b.colony,
                ));
            }
            if b.def.light > 0.0 {
                let cx = b.col as f64 + b.w as f64 / 2.0;
                let cy = b.row as f64 + b.h as f64 / 2.0;
                self.light_sources.push((cx, cy, b.def.light * b.def.light));
            }
        }
    }

    pub fn well_coverage(&self, pi: usize) -> f64 {
        let (x, y) = (self.people[pi].x, self.people[pi].y);
        let colony = self.people[pi].colony;
        let mut best: f64 = 0.0;
        for &(col, row, radius, health, bcolony) in &self.health_sources {
            if bcolony != colony {
                continue;
            }
            let d = (col - x).hypot(row - y);
            if d <= radius {
                best = best.max(health);
            }
        }
        best
    }

    pub fn average_happiness(&self) -> f64 {
        let n = self.people.count();
        if n == 0 {
            return 0.0;
        }
        self.people.iter().map(|p| p.happiness).sum::<f64>() / n as f64
    }

    pub fn colony_happiness(&self, colony: i32) -> f64 {
        let mut sum = 0.0;
        let mut n = 0;
        for p in self.people.iter().filter(|p| p.colony == colony) {
            sum += p.happiness;
            n += 1;
        }
        if n == 0 {
            0.0
        } else {
            sum / n as f64
        }
    }

    // ---- tasks -----------------------------------------------------------

    pub fn plant_mass(&self, plant: &Plant) -> f64 {
        let cell_px = self.world().cell_px as f64;
        (plant.height_px + plant.radius_px * 2.0) / cell_px
    }

    /// Refills the coarse plant buckets, and with them the grid of cells a
    /// plant is standing in the way in. Everything that asks "what is growing
    /// near here" reads these rather than the plant list, and so does the
    /// pathfinder, which cannot afford to ask anything else.
    pub fn rebuild_plant_index(&mut self) {
        let (cols, rows) = (self.world().cols, self.world().rows);
        if self.plant_index.cols == 0 || self.plant_index.buckets.is_empty() {
            self.plant_index.resize(cols, rows);
        }
        if self.plant_block.len() != (cols * rows) as usize {
            self.plant_block = vec![0; (cols * rows) as usize];
        } else {
            self.plant_block.fill(0);
        }
        let block_mass = self.block_mass;
        let mut index = std::mem::take(&mut self.plant_index);
        for bucket in &mut index.buckets {
            bucket.clear();
        }
        index.slot_of.clear();
        let cell_px = self.world().cell_px as f64;
        // Usually empty, and never more than a handful of entries: a scan over
        // it beats hashing a species name once per plant on the map.
        let taught = self.lore.known();
        for plant in &self.plant_sim.plants {
            if !plant.standing() {
                continue;
            }
            let mass = ((plant.height_px + plant.radius_px * 2.0) / cell_px) as f32;
            let lore = taught
                .iter()
                .find(|(id, _)| *id == plant.species_id)
                .map(|&(_, interest)| interest as f32)
                .unwrap_or(0.0);
            let b = index.bucket_of(plant.col, plant.row);
            let slot = index.buckets[b].len();
            index.buckets[b].push(PlantMark {
                id: plant.id,
                col: plant.col,
                row: plant.row,
                mass,
                height_px: plant.height_px as f32,
                radius_px: plant.radius_px as f32,
                class: plant.size_class,
                claimed_by: plant.claimed_by,
                lore,
            });
            index.slot_of.insert(plant.id, (b, slot));
            // Only the cell the stem is in: a canopy is walked under, and a
            // wood that shut every cell it shaded would be a wall. Only
            // something with a stem, either: a mat or a tuft is trodden over,
            // however much of it there is.
            let woody = matches!(
                plant.size_class,
                SizeClass::Shrub | SizeClass::Tree | SizeClass::Vine
            );
            if woody && block_mass > 0.0 && mass as f64 >= block_mass {
                let i = (plant.row * cols + plant.col) as usize;
                self.plant_block[i] = 1;
            }
        }
        self.plant_index = index;
    }

    pub fn claim_plant(&mut self, id: i32, by: u32) {
        self.plant_index.set_claim(id, by);
    }

    pub fn site_ready(&self, bi: usize) -> bool {
        let site = &self.buildings[bi];
        for &(res, n) in &site.cost {
            if site.delivered[res as usize] < n {
                return false;
            }
        }
        site.work_done < site.work
    }

    pub fn craft_ready(&self, bi: usize) -> bool {
        let b = &self.buildings[bi];
        match &b.def.job {
            Some(Job::Craft { input, .. }) => {
                input.iter().all(|&(res, n)| b.inv[res as usize] >= n)
            }
            _ => false,
        }
    }

    pub fn free_cell_near(&self, col: i32, row: i32) -> Option<(i32, i32)> {
        if self.walkable(col, row) {
            return Some((col, row));
        }
        for radius in 1..=3 {
            for r in row - radius..=row + radius {
                for c in col - radius..=col + radius {
                    if self.walkable(c, r) {
                        return Some((c, r));
                    }
                }
            }
        }
        None
    }

    /// Workers stand around their building rather than inside it, spread along
    /// the free cells next to it so a crowded workshop still reads clearly.
    pub fn work_spot(&self, bi: usize, person_id: u32) -> (i32, i32) {
        let at = self.access_cell(bi);
        let slot = self.buildings[bi].workers.iter().position(|&w| w == person_id);
        let slot = match slot {
            Some(s) if s > 0 => s,
            _ => return at,
        };
        const OFFSETS: [(i32, i32); 6] = [(0, 0), (1, 0), (-1, 0), (0, 1), (1, 1), (-1, 1)];
        let (dx, dy) = OFFSETS[slot % OFFSETS.len()];
        let c = at.0 + dx;
        let r = at.1 + dy;
        if self.walkable(c, r) {
            (c, r)
        } else {
            at
        }
    }

    /// The share of a farm's fields that lie within reach of open water. A
    /// farm on a riverbank waters itself; one out in the dry has to be carried
    /// to.
    pub fn farm_soak(&self, state: &State, bi: usize) -> f64 {
        let b = &self.buildings[bi];
        let rad = if b.def.fields > 0 { b.def.fields } else { 2 };
        let reach = state.civ.work.farm_soak_reach.max(0);
        let mut wet = 0;
        let mut n = 0;
        for r in b.row - rad..=b.row + b.h + rad {
            for c in b.col - rad..=b.col + b.w + rad {
                if !self.in_bounds(c, r) {
                    continue;
                }
                n += 1;
                if self.water_within(c, r, reach) {
                    wet += 1;
                }
            }
        }
        if n > 0 {
            wet as f64 / n as f64
        } else {
            0.0
        }
    }

    /// Whether open water lies within `reach` cells. A square search: this is
    /// asked once per farm per tick, not once per cell.
    fn water_within(&self, col: i32, row: i32, reach: i32) -> bool {
        for r in row - reach..=row + reach {
            for c in col - reach..=col + reach {
                if self.in_bounds(c, r) && self.in_water(c, r) {
                    return true;
                }
            }
        }
        false
    }

    /// The nearest cell of open water to somewhere, for whoever is going to
    /// fill a bucket. None if there is none within a day's walk of the town.
    pub fn nearest_water(&self, col: i32, row: i32, reach: i32) -> Option<(i32, i32)> {
        for ring in 0..=reach {
            let mut best: Option<((i32, i32), i32)> = None;
            for r in row - ring..=row + ring {
                for c in col - ring..=col + ring {
                    if r.abs_diff(row) as i32 != ring && c.abs_diff(col) as i32 != ring {
                        continue;
                    }
                    if !self.in_bounds(c, r) || !self.in_water(c, r) {
                        continue;
                    }
                    // A bank to stand on, not the middle of a lake.
                    let bank = self.free_spot_near(c, r)?;
                    let d = (bank.0 - col).abs() + (bank.1 - row).abs();
                    if best.is_none_or(|(_, was)| d < was) {
                        best = Some((bank, d));
                    }
                }
            }
            if let Some((bank, _)) = best {
                return Some(bank);
            }
        }
        None
    }

    /// How much of its yield a farm is bringing in, given how wet it is.
    pub fn farm_water_factor(&self, state: &State, bi: usize) -> f64 {
        let dry = clamp01(state.civ.work.farm_dry_yield);
        dry + (1.0 - dry) * clamp01(self.buildings[bi].water)
    }

    pub fn farm_fertility(&self, bi: usize) -> f64 {
        let b = &self.buildings[bi];
        let rad = if b.def.fields > 0 { b.def.fields } else { 2 };
        let mut sum = 0.0;
        let mut n = 0;
        for r in b.row - rad..=b.row + b.h + rad {
            for c in b.col - rad..=b.col + b.w + rad {
                if !self.in_bounds(c, r) {
                    continue;
                }
                sum += self.terrain.fertility(c, r);
                n += 1;
            }
        }
        if n > 0 {
            clamp(0.25 + (sum / n as f64) * 1.5, 0.1, 2.5)
        } else {
            0.4
        }
    }

    // ---- production, economy, research -----------------------------------

    /// Workshops with nobody in them slowly lose their half made goods, which
    /// keeps abandoned buildings from holding stock hostage.
    fn production_tick(&mut self) {
        for b in &mut self.buildings {
            if !b.built {
                continue;
            }
            if matches!(b.def.job, Some(Job::Craft { .. })) && b.workers.is_empty() && b.craft_progress > 0.0
            {
                b.craft_progress = (b.craft_progress - 0.02).max(0.0);
            }
        }
    }

    fn economy_tick(&mut self, state: &State, ci: usize, dt: f64) {
        let cfg = &state.civ.economy;
        let colony = self.colonies[ci].id;
        let pop = self.colony_population(colony);
        {
            let c = &mut self.colonies[ci];
            update_prices(&mut c.econ, cfg, &c.stock, pop, dt);
        }
        if !self.has_market(colony) {
            return;
        }
        self.colonies[ci].econ.trade_timer += dt;
        if self.colonies[ci].econ.trade_timer < cfg.trade_interval {
            return;
        }
        self.colonies[ci].econ.trade_timer = 0.0;
        let day = self.day;
        let mods = self.colonies[ci].mods;
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        let c = &mut self.colonies[ci];
        run_caravan(&mut c.econ, cfg, &mut c.stock, pop, &mods, &mut rng, day);
        self.rng = rng;
    }

    fn research_tick(&mut self, state: &State, ci: usize, dt: f64) {
        let cfg = &state.civ.tech;
        let colony = self.colonies[ci].id;
        let pop = self.colony_population(colony) as f64;
        let mods_research = self.colonies[ci].mods.research;
        // Anything the town has up in the air over it, which is one for a town
        // with no experiment switched on.
        let aloft = crate::civ::balloons::research_lift(self, state, colony);
        self.colonies[ci].tech.points += pop * cfg.insight_per_person * mods_research * aloft * dt;
        let mut target: Option<&'static TechDef> = self.colonies[ci]
            .tech
            .target
            .as_deref()
            .filter(|id| !self.colonies[ci].tech.is_known(id))
            .and_then(tech_by_id);
        if let Some(def) = target {
            if !self.colonies[ci].tech.reachable(def) {
                target = None;
            }
        }
        if target.is_none() && cfg.auto_research {
            target = self.pick_research(state, ci);
        }
        let target = match target {
            Some(t) => t,
            None => return,
        };
        let cost = tech_cost(target, cfg);
        if self.colonies[ci].tech.points < cost {
            return;
        }
        let day = self.day;
        let c = &mut self.colonies[ci];
        c.tech.points -= cost;
        c.tech.spent += cost;
        c.tech.known.push(target.id.to_string());
        c.tech.log.push((target.id.to_string(), day));
        if c.tech.target.as_deref() == Some(target.id) {
            c.tech.target = None;
        }
        c.refresh_tech();
        c.econ.log_event(format!("learned {}", target.label), day);
    }

    /// Cheapest reachable tech, nudged toward whatever unlocks something the
    /// colony is currently short of.
    fn pick_research(&self, state: &State, ci: usize) -> Option<&'static TechDef> {
        let cfg = &state.civ.tech;
        let options = self.colonies[ci].tech.available();
        if options.is_empty() {
            return None;
        }
        let colony = self.colonies[ci].id;
        let pop = self.colony_population(colony);
        let targets = stock_targets(&state.civ.economy, pop);
        let stock = &self.colonies[ci].stock;
        let mut best = None;
        let mut best_score = f64::NEG_INFINITY;
        for t in options {
            let mut score = -tech_cost(t, cfg) / 100.0;
            let mut need = 0.0;
            for bid in t.unlocks {
                let def = match building_by_id(bid) {
                    Some(d) => d,
                    None => continue,
                };
                if def.housing > 0
                    && self.housing_capacity(colony) < pop as i32 + state.civ.build.housing_slack
                {
                    need += 1.0;
                }
                if def.is_inn && self.roofless(colony) > 1 {
                    need += 1.2;
                }
                if def.is_dock && self.colonies.len() > 1 {
                    need += 1.0;
                }
                // A town wants a wall once there is enough of it to be worth
                // walling, and wants counters once there is a market for
                // anybody to have coin from.
                if def.structure.perimeter()
                    && state.civ.build.walls
                    && pop as i32 >= state.civ.build.wall_population
                    && self.count_all(colony, def.id) == 0
                {
                    need += 1.2;
                }
                if def.structure == Structure::Stall
                    && state.civ.build.stalls
                    && pop as i32 >= state.civ.build.stall_customers
                {
                    need += 0.6;
                }
                let job = match &def.job {
                    Some(j) => j,
                    None => continue,
                };
                for &(res, _) in job.produces() {
                    let short = clamp(
                        (targets[res as usize] - stock[res as usize]) / targets[res as usize].max(1.0),
                        0.0,
                        1.0,
                    );
                    // A hungry colony should be reaching for agriculture, not
                    // for whatever happens to be cheapest.
                    need += if res == Res::Food { short * 3.0 } else { short };
                }
            }
            score += need * (1.0 + cfg.need_bias);
            if score > best_score {
                best_score = score;
                best = Some(t);
            }
        }
        best
    }

    // ---- the build planner -----------------------------------------------

    /// Queued by hand from the build panel: same placement rules, no planner.
    pub fn queue_building(&mut self, state: &State, ci: usize, type_id: &str) -> Option<usize> {
        if ci >= self.colonies.len() {
            return None;
        }
        let def = building_by_id(type_id)?;
        // A piece of wall belongs on the ring rather than wherever there
        // happens to be room, so the Build button places the piece the wall
        // planner would have placed next.
        let site = if def.structure.perimeter() {
            ring_site(self, state, ci, def)?
        } else {
            find_site(self, state, ci, def)?
        };
        self.place_building(state, ci, type_id, site.0, site.1, false)
    }

    // ---- reporting -------------------------------------------------------

    pub fn stats(&self, state: &State) -> Stats {
        let mut professions: Vec<(Profession, usize)> = Vec::new();
        let mut children = 0;
        for p in self.people.iter() {
            match professions.iter_mut().find(|(prof, _)| *prof == p.profession) {
                Some(entry) => entry.1 += 1,
                None => professions.push((p.profession, 1)),
            }
            if !p.adult() {
                children += 1;
            }
        }
        let colonies = self.colonies.iter().map(|c| self.colony_stats(c)).collect();
        let total = self.total_stock();
        Stats {
            name: self.name.clone(),
            day: self.day,
            day_fraction: day_fraction(self.time, &state.civ.people),
            daylight: daylight(self.time, &state.civ.people),
            population: self.people.count(),
            children,
            professions,
            housing: self.colonies.iter().map(|c| self.housing_capacity(c.id)).sum(),
            buildings: self.buildings.iter().filter(|b| b.built).count(),
            sites: self.sites().len(),
            storage: self.colonies.iter().map(|c| self.store_capacity(c.id)).sum(),
            bulk: stock_bulk(&total),
            coin: self.colonies.iter().map(|c| c.econ.coin).sum(),
            research: self.colonies.iter().map(|c| c.tech.points).sum(),
            known: self.colonies.iter().map(|c| c.tech.known.len()).max().unwrap_or(0),
            techs: TECHS.len(),
            births: self.births,
            deaths: self.deaths,
            happiness: self.average_happiness(),
            time: self.time,
            ticks: self.ticks,
            colonies,
            boats: self.boats.len(),
        }
    }

    fn colony_stats(&self, c: &Colony) -> ColonyStats {
        let mut population = 0;
        let mut children = 0;
        let mut wealth = 0.0;
        for p in self.people.iter().filter(|p| p.colony == c.id) {
            population += 1;
            if !p.adult() {
                children += 1;
            }
            wealth += p.coin;
        }
        ColonyStats {
            id: c.id,
            name: c.name.clone(),
            population,
            children,
            housing: self.housing_capacity(c.id),
            buildings: self.buildings.iter().filter(|b| b.built && b.colony == c.id).count(),
            sites: self.colony_sites(c.id),
            coin: c.econ.coin,
            food: c.stock[Res::Food as usize],
            known: c.tech.known.len(),
            happiness: self.colony_happiness(c.id),
            wealth,
            center: c.center,
        }
    }

    /// The richest settlers, for the roster and for deciding who is worth a
    /// tower.
    pub fn wealthiest(&self, n: usize) -> Vec<usize> {
        let mut all: Vec<usize> = self.people.live_indices();
        all.sort_by(|&a, &b| {
            self.people[b]
                .coin
                .partial_cmp(&self.people[a].coin)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all.truncate(n);
        all
    }

    // ---- drawing ---------------------------------------------------------

    pub fn daylight(&self, state: &State) -> f64 {
        daylight(self.time, &state.civ.people)
    }

    /// Whether a lamp reaches this cell. Only built lamps count: a post going
    /// up is not lighting anything yet.
    pub fn lit_at(&self, col: i32, row: i32) -> bool {
        self.light_sources.iter().any(|&(cx, cy, r2)| {
            let dx = col as f64 + 0.5 - cx;
            let dy = row as f64 + 0.5 - cy;
            dx * dx + dy * dy <= r2
        })
    }

    /// Windows are lit once it is dark enough to want them.
    pub fn night_lights(&self, state: &State) -> bool {
        self.daylight(state) < 0.4
    }

    pub fn composite(&mut self, state: &State) {
        composite_settlement(self, state);
    }

    /// The sampling boxes or the cell size changed, so every generated sprite
    /// has to be built again.
    pub fn invalidate_sprites(&mut self) {
        self.sprites.clear();
        self.bg.clear();
        self.bg_key.clear();
        self.ground_dirty = true;
        self.buffer_dirty = true;
    }

    /// Compatibility with the plant sim view: the settlement is rasterized by
    /// the same viewport, so it answers the same two questions.
    pub fn process_raster_queue(&mut self, state: &State, budget: usize) -> usize {
        let _t = phases::time(Phase::Raster);
        self.plant_sim.process_raster_queue(state, budget)
    }

    pub fn mark_all_dirty(&mut self) {
        self.plant_sim.mark_all_dirty();
        self.buffer_dirty = true;
    }
}

pub fn profession_for(def: &BuildingDef) -> Profession {
    match &def.job {
        Some(Job::Harvest { yields, .. }) => {
            if yields.iter().any(|&(res, _)| res == Res::Wood) {
                Profession::Woodcutter
            } else {
                Profession::Forager
            }
        }
        Some(Job::Mine { .. }) => Profession::Miner,
        Some(Job::Farm { .. }) => Profession::Farmer,
        Some(Job::Craft { .. }) => Profession::Crafter,
        Some(Job::Research) => Profession::Scholar,
        Some(Job::Trade) => Profession::Trader,
        Some(Job::Innkeep) => Profession::Innkeeper,
        Some(Job::Ferry) => Profession::Sailor,
        Some(Job::Sell) => Profession::Shopkeeper,
        _ => Profession::Laborer,
    }
}

/// A settler's standing in their colony, which is what the roster sorts by and
/// what decides whether a tower is worth anybody's while.
pub fn standing(sim: &Settlement, pi: usize) -> f64 {
    let p = &sim.people[pi];
    let house = sim
        .building_index(p.owns)
        .map(|bi| sim.buildings[bi].def.prestige + home_rank(sim.buildings[bi].def) as f64 * 0.15)
        .unwrap_or(0.0);
    let counter = if sim.building_index(p.stall).is_some() { 0.3 } else { 0.0 };
    // Being liked is worth something in a town small enough that everybody
    // knows everybody.
    let regard = (p.friends as f64 - p.rivals as f64) * 0.05;
    p.coin / 200.0 + house + counter + regard + p.skill * 0.2
}
