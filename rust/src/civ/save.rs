//! A running settlement, written down and picked up again.
//!
//! What is saved is only what could not be worked out again. The terrain, the
//! walkability grids, the plant sprites and every cached picture are all
//! functions of the map seed, the configuration and the things that are saved,
//! so they are left out of the file and rebuilt on the way back in. What is
//! left is the part with history in it: the people, what they own, what they
//! know, the shapes the wilderness has grown into, and the random streams that
//! got it all there.
//!
//! A file is only good for the world it grew on. `world_key` is the same
//! string the restart bar compares, so a settlement saved before the map size
//! or the terrain settings were changed is discarded rather than dropped onto
//! ground that no longer matches it.

use serde::{Deserialize, Serialize};

use crate::civ::boats::Boat;
use crate::civ::colony::Colony;
use crate::civ::harvest::Lore;
use crate::civ::settlement::{Building, Obituary, Pile, Settlement};
use crate::civ::people_db::PeopleDb;
use crate::civ::terrain::Cell;
use crate::plant::Plant;
use crate::rng::Rng;
use crate::state::State;

/// Bumped whenever the shape below changes in a way an older file cannot be
/// read into. There is no upgrade path: a settlement is a thing being watched,
/// not a document, and starting a fresh one costs a moment.
pub const SNAPSHOT_VERSION: u32 = 3;

/// The world a saved settlement grew on, as one string. Everything the map is
/// built from is in here, so two settlements with the same key stand on the
/// same ground.
pub fn world_key(state: &State) -> String {
    serde_json::to_string(&(&state.civ.world, &state.civ.terrain, state.civ.seed))
        .unwrap_or_default()
}

#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub world_key: String,

    // ---- the wilderness ----
    pub plants: Vec<Plant>,
    pub plant_rng: Rng,
    pub plant_next_id: i32,
    pub plant_time: f64,
    pub plant_ticks: u64,
    pub wild_scale: f64,
    /// How long the coarse plant index has left before its next sweep.
    pub plant_index_timer: f64,

    // ---- the settlement ----
    pub rng: Rng,
    pub buildings: Vec<Building>,
    pub next_building_id: i32,
    pub piles: Vec<Pile>,
    pub next_pile_id: i32,
    pub people: PeopleDb,
    pub colonies: Vec<Colony>,
    pub next_colony_id: i32,
    pub focus: usize,
    /// What cutting by hand has taught the gatherers. Not derivable from
    /// anything else in the file: it is a record of what was asked for.
    #[serde(default)]
    pub lore: Lore,
    pub boats: Vec<Boat>,
    pub next_boat_id: i32,
    /// Where the ground has been walked into paths.
    pub traffic: Vec<f32>,
    /// Which cells a plant is standing in the way in. Worked out from the
    /// plants, but only on a timer, and the pathfinder reads it: a restored
    /// settlement that rebuilt it a second early would walk round a tree the
    /// saved one had not noticed yet, and come apart from there.
    #[serde(default)]
    pub plant_block: Vec<u8>,
    /// What is left in each deposit, in the order the terrain lays them out.
    /// The rest of the map is made fresh from the seed.
    pub deposits: Vec<f64>,
    pub time: f64,
    pub day: i32,
    pub ticks: u64,
    pub traffic_timer: f64,
    pub social_timer: f64,
    pub births: u32,
    pub deaths: u32,
    pub dead: Vec<Obituary>,
    pub name: String,
    pub center: Option<(i32, i32)>,
    pub warmup_done: f64,
    pub extinct_at: Option<f64>,
}

/// The same shape as `Snapshot`, borrowed rather than owned, so writing a
/// settlement down does not first copy every plant and every settler in it.
/// Serde matches the two by field name.
#[derive(Serialize)]
struct SnapshotRef<'a> {
    version: u32,
    world_key: String,

    plants: &'a [Plant],
    plant_rng: &'a Rng,
    plant_next_id: i32,
    plant_time: f64,
    plant_ticks: u64,
    wild_scale: f64,
    plant_index_timer: f64,

    rng: &'a Rng,
    buildings: &'a [Building],
    next_building_id: i32,
    piles: &'a [Pile],
    next_pile_id: i32,
    people: &'a PeopleDb,
    colonies: &'a [Colony],
    next_colony_id: i32,
    focus: usize,
    lore: &'a Lore,
    boats: &'a [Boat],
    next_boat_id: i32,
    traffic: &'a [f32],
    plant_block: &'a [u8],
    deposits: Vec<f64>,
    time: f64,
    day: i32,
    ticks: u64,
    traffic_timer: f64,
    social_timer: f64,
    births: u32,
    deaths: u32,
    dead: &'a [Obituary],
    name: &'a str,
    center: Option<(i32, i32)>,
    warmup_done: f64,
    extinct_at: Option<f64>,
}

/// Writes a settlement out. Everything that can be worked out again is left
/// behind, so this is roughly the size of the history in it.
pub fn capture(sim: &Settlement, state: &State) -> String {
    let snap = SnapshotRef {
        version: SNAPSHOT_VERSION,
        world_key: world_key(state),
        plants: &sim.plant_sim.plants,
        plant_rng: &sim.plant_sim.rng,
        plant_next_id: sim.plant_sim.next_id,
        plant_time: sim.plant_sim.time,
        plant_ticks: sim.plant_sim.ticks,
        wild_scale: sim.plant_sim.wild_scale,
        plant_index_timer: sim.plant_index.timer,
        rng: &sim.rng,
        buildings: &sim.buildings,
        next_building_id: sim.next_building_id,
        piles: &sim.piles,
        next_pile_id: sim.next_pile_id,
        people: &sim.people,
        colonies: &sim.colonies,
        next_colony_id: sim.next_colony_id,
        focus: sim.focus,
        lore: &sim.lore,
        boats: &sim.boats,
        next_boat_id: sim.next_boat_id,
        traffic: &sim.traffic,
        plant_block: &sim.plant_block,
        deposits: sim.terrain.deposits.iter().map(|d| d.amount).collect(),
        time: sim.time,
        day: sim.day,
        ticks: sim.ticks,
        traffic_timer: sim.traffic_timer,
        social_timer: sim.social_timer,
        births: sim.births,
        deaths: sim.deaths,
        dead: &sim.dead,
        name: &sim.name,
        center: sim.center,
        warmup_done: sim.warmup_done,
        extinct_at: sim.extinct_at,
    };
    serde_json::to_string(&snap).unwrap_or_default()
}

impl Snapshot {
    pub fn from_json(raw: &str) -> Result<Snapshot, String> {
        let snap: Snapshot = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        if snap.version != SNAPSHOT_VERSION {
            return Err(format!("settlement saved by another version ({})", snap.version));
        }
        Ok(snap)
    }

    /// Whether this file is about the world the project would build now.
    pub fn fits(&self, state: &State) -> bool {
        self.world_key == world_key(state)
    }
}

/// Puts a settlement back the way it was left. The map is made again from the
/// seed first, so what is written over it is only ever the part with history.
///
/// Fails rather than half restores: a file for another world would leave
/// people standing in a river.
pub fn restore(sim: &mut Settlement, state: &State, snap: Snapshot) -> Result<(), String> {
    if !snap.fits(state) {
        return Err("the saved settlement is for a different world".to_string());
    }
    sim.reset(state, state.civ.seed);

    // ---- the wilderness, and the cells it claims ----
    sim.plant_sim.rng = snap.plant_rng;
    sim.plant_sim.next_id = snap.plant_next_id;
    sim.plant_sim.time = snap.plant_time;
    sim.plant_sim.ticks = snap.plant_ticks;
    sim.plant_sim.wild_scale = snap.wild_scale;
    sim.plant_sim.plants = snap.plants;
    for i in 0..sim.plant_sim.plants.len() {
        sim.plant_sim.plants[i].rehydrate();
        let (layer, id) = (sim.plant_sim.plants[i].layer, sim.plant_sim.plants[i].id);
        let cells = std::mem::take(&mut sim.plant_sim.plants[i].cells);
        sim.plant_sim.world.claim(layer, &cells, id);
        sim.plant_sim.plants[i].cells = cells;
        sim.plant_sim.raster_queue.push_back(id);
    }
    sim.plant_sim.buffer_dirty = true;

    // ---- what was dug out of the ground ----
    for (i, amount) in snap.deposits.iter().enumerate() {
        if i >= sim.terrain.deposits.len() {
            break;
        }
        sim.terrain.deposits[i].amount = *amount;
        if *amount <= 0.0 {
            let (col, row) = (sim.terrain.deposits[i].col, sim.terrain.deposits[i].row);
            let at = sim.terrain.idx(col, row);
            sim.terrain.deposit_index[at] = 0;
        }
    }

    // ---- the town ----
    sim.rng = snap.rng;
    sim.buildings = snap.buildings;
    sim.next_building_id = snap.next_building_id;
    sim.piles = snap.piles;
    sim.next_pile_id = snap.next_pile_id;
    sim.people = snap.people;
    sim.colonies = snap.colonies;
    sim.next_colony_id = snap.next_colony_id;
    sim.focus = snap.focus.min(sim.colonies.len().saturating_sub(1));
    // Read back before the plant buckets are filled: every mark carries what
    // the towns have been taught about its species.
    sim.lore = snap.lore;
    sim.boats = snap.boats;
    sim.next_boat_id = snap.next_boat_id;
    if snap.traffic.len() == sim.traffic.len() {
        sim.traffic = snap.traffic;
    }
    sim.time = snap.time;
    sim.day = snap.day;
    sim.ticks = snap.ticks;
    sim.traffic_timer = snap.traffic_timer;
    sim.social_timer = snap.social_timer;
    sim.births = snap.births;
    sim.deaths = snap.deaths;
    sim.dead = snap.dead;
    sim.name = snap.name;
    sim.center = snap.center;
    sim.warmup_done = snap.warmup_done;
    sim.extinct_at = snap.extinct_at;

    // ---- everything the above is enough to work out again ----
    sim.reindex_buildings();
    rebuild_ground(sim);
    for colony in &mut sim.colonies {
        colony.refresh_tech();
    }
    sim.people.reindex();
    sim.rebuild_plant_index();
    sim.plant_index.timer = snap.plant_index_timer;
    // After the rebuild, which would otherwise have worked out a fresher one
    // than the settlement was saved with.
    if snap.plant_block.len() == sim.plant_block.len() {
        sim.plant_block = snap.plant_block;
    }
    sim.refresh_colonies();
    sim.ready = true;
    sim.ground_dirty = true;
    sim.buffer_dirty = true;
    Ok(())
}

/// What the buildings do to the map: the ground they stand on is blocked and
/// carries their id, and a gate that is finished is a way through the wall it
/// sits in.
fn rebuild_ground(sim: &mut Settlement) {
    let cols = sim.world().cols;
    for i in 0..sim.blocked.len() {
        sim.blocked[i] = u8::from(sim.terrain.kind[i] == Cell::Water as u8);
        sim.build_grid[i] = 0;
        sim.gates[i] = 0;
    }
    for bi in 0..sim.buildings.len() {
        let b = &sim.buildings[bi];
        let (id, col, row, w, h) = (b.id, b.col, b.row, b.w, b.h);
        let gate = b.built && b.def.structure.passable();
        for r in row..row + h {
            for c in col..col + w {
                if !sim.in_bounds(c, r) {
                    continue;
                }
                let i = (r * cols + c) as usize;
                sim.build_grid[i] = id;
                sim.blocked[i] = 1;
                if gate {
                    sim.gates[i] = 1;
                }
            }
        }
    }
}
