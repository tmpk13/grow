//! Boats and the water between colonies.
//!
//! A river is only a feature until something uses it. A colony with a dock
//! builds boats there, crews them from the dock workers, and sends them to the
//! other colonies with whatever it has too much of. The boat sells into the far
//! colony's store at the far colony's prices, buys back what home is short of,
//! and comes home. Two towns on the same river therefore level each other out
//! without either of them planning it.
//!
//! Boats never touch land. They path over water cells only, which is why a
//! colony on a lake trades with nobody and a colony on a river that reaches the
//! sea trades with everyone.

use crate::civ::economy::stock_targets;
use crate::civ::names::boat_name;
use crate::civ::resources::{add_stock, take_stock, Res, Stock, RES_COUNT, RES_IDS};
use crate::civ::settlement::Settlement;
use crate::civ::terrain::Cell;
use crate::state::State;
use crate::util::clamp;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoatState {
    /// Tied up at its home dock, waiting for a cargo worth the trip.
    Moored,
    Outbound,
    /// Trading at the far dock.
    Trading,
    Inbound,
}

impl BoatState {
    pub fn label(self) -> &'static str {
        match self {
            BoatState::Moored => "moored",
            BoatState::Outbound => "outbound",
            BoatState::Trading => "trading",
            BoatState::Inbound => "inbound",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BoatConfig {
    /// Boats a colony will keep, per dock.
    pub per_dock: i32,
    pub capacity: f64,
    pub speed: f64,
    /// What it costs the colony to lay down a hull.
    pub hull_wood: f64,
    pub hull_plank: f64,
    /// Simulated seconds spent at each end of a voyage.
    pub port_time: f64,
    /// Cargo below this is not worth casting off for.
    pub min_cargo: f64,
    /// Margin the far colony's market takes on both halves of the trade.
    pub margin: f64,
    pub crew: i32,
}

impl Default for BoatConfig {
    fn default() -> Self {
        BoatConfig {
            per_dock: 2,
            capacity: 90.0,
            speed: 2.6,
            hull_wood: 12.0,
            hull_plank: 10.0,
            port_time: 14.0,
            min_cargo: 12.0,
            margin: 0.12,
            crew: 2,
        }
    }
}

pub struct Boat {
    pub id: i32,
    pub name: String,
    pub colony: i32,
    pub home_dock: i32,
    pub dest_dock: i32,
    pub dest_colony: i32,
    pub x: f64,
    pub y: f64,
    pub path: Vec<(i32, i32)>,
    pub path_at: usize,
    pub cargo: Stock,
    pub coin: f64,
    pub crew: Vec<u32>,
    pub state: BoatState,
    pub wait: f64,
    pub seed: u32,
    pub facing: i32,
    pub voyages: u32,
    pub bob: f64,
}

impl Boat {
    pub fn load(&self) -> f64 {
        self.cargo.iter().sum()
    }

    pub fn cell(&self) -> (i32, i32) {
        (self.x.floor() as i32, self.y.floor() as i32)
    }

    /// Advances along the current course. True once the last cell is reached.
    fn sail(&mut self, dt: f64, speed: f64) -> bool {
        if self.path.is_empty() {
            return true;
        }
        let mut budget = speed * dt;
        while budget > 0.0 && !self.path.is_empty() {
            let node = self.path[self.path_at];
            let tx = node.0 as f64 + 0.5;
            let ty = node.1 as f64 + 0.5;
            let dx = tx - self.x;
            let dy = ty - self.y;
            let d = dx.hypot(dy);
            if d <= budget || d < 1e-4 {
                self.x = tx;
                self.y = ty;
                budget -= d;
                self.path_at += 1;
                if self.path_at >= self.path.len() {
                    self.path.clear();
                    self.path_at = 0;
                    return true;
                }
            } else {
                self.x += (dx / d) * budget;
                self.y += (dy / d) * budget;
                if dx.abs() > 0.01 {
                    self.facing = if dx > 0.0 { 1 } else { -1 };
                }
                budget = 0.0;
            }
        }
        self.bob += dt;
        self.path.is_empty()
    }
}

/// The water cell a boat ties up at for a dock, which is the nearest navigable
/// cell to the jetty.
pub fn mooring_cell(sim: &Settlement, bi: usize) -> Option<(i32, i32)> {
    let b = &sim.buildings[bi];
    let mut best = None;
    let mut best_d = f64::INFINITY;
    let reach = 4;
    for r in b.row - reach..=b.row + b.h + reach {
        for c in b.col - reach..=b.col + b.w + reach {
            if !sim.terrain.navigable(c, r) {
                continue;
            }
            let dx = (c - b.col) as f64;
            let dy = (r - b.row) as f64;
            let d = dx * dx + dy * dy;
            if d < best_d {
                best_d = d;
                best = Some((c, r));
            }
        }
    }
    best
}

/// A course over open water. Boats ignore roads, so the wear term is zero.
pub fn water_path(sim: &mut Settlement, from: (i32, i32), to: (i32, i32)) -> Option<Vec<(i32, i32)>> {
    let mut grid = std::mem::take(&mut sim.water_paths);
    let kind = &sim.terrain.kind;
    let cols = sim.terrain.cols;
    let rows = sim.terrain.rows;
    let navigable = |c: i32, r: i32| {
        c >= 0 && c < cols && r >= 0 && r < rows && kind[(r * cols + c) as usize] == Cell::Water as u8
    };
    let out = grid.find(from, to, 60_000, navigable, |_| 0.0);
    sim.water_paths = grid;
    out
}

// ---- the fleet -----------------------------------------------------------

/// Boats are laid down at a dock that has the hull materials for one, up to the
/// per dock limit. This is the only way a boat ever comes into existence.
pub fn build_boats(sim: &mut Settlement, state: &State) {
    let cfg = state.civ.boats;
    if cfg.per_dock <= 0 {
        return;
    }
    let docks: Vec<usize> = (0..sim.buildings.len())
        .filter(|&i| sim.buildings[i].built && sim.buildings[i].def.is_dock)
        .collect();
    for bi in docks {
        let (dock_id, colony) = (sim.buildings[bi].id, sim.buildings[bi].colony);
        let have = sim.boats.iter().filter(|b| b.home_dock == dock_id).count() as i32;
        if have >= cfg.per_dock {
            continue;
        }
        let ci = match sim.colony_index(colony) {
            Some(ci) if sim.is_live(ci) => ci,
            _ => continue,
        };
        if sim.colonies[ci].available(Res::Wood) < cfg.hull_wood
            || sim.colonies[ci].available(Res::Plank) < cfg.hull_plank
        {
            continue;
        }
        let moor = match mooring_cell(sim, bi) {
            Some(m) => m,
            None => continue,
        };
        take_stock(&mut sim.colonies[ci].stock, Res::Wood, cfg.hull_wood);
        take_stock(&mut sim.colonies[ci].stock, Res::Plank, cfg.hull_plank);
        let id = sim.next_boat_id;
        sim.next_boat_id += 1;
        let seed = sim.rng.seed();
        let name = boat_name(&mut sim.rng);
        let day = sim.day;
        sim.colonies[ci]
            .econ
            .log_event(format!("{name} was launched"), day);
        sim.boats.push(Boat {
            id,
            name,
            colony,
            home_dock: dock_id,
            dest_dock: 0,
            dest_colony: 0,
            x: moor.0 as f64 + 0.5,
            y: moor.1 as f64 + 0.5,
            path: Vec::new(),
            path_at: 0,
            cargo: [0.0; RES_COUNT],
            coin: 0.0,
            crew: Vec::new(),
            state: BoatState::Moored,
            wait: cfg.port_time,
            seed,
            facing: 1,
            voyages: 0,
            bob: 0.0,
        });
    }
}

pub fn boats_tick(sim: &mut Settlement, state: &State, dt: f64) {
    let cfg = state.civ.boats;
    for i in 0..sim.boats.len() {
        step_boat(sim, state, i, dt, &cfg);
    }
    // Crews ride with the hull rather than walking their own paths.
    for i in 0..sim.boats.len() {
        let (x, y, crew) = {
            let boat = &sim.boats[i];
            (boat.x, boat.y, boat.crew.clone())
        };
        for id in crew {
            if let Some(pi) = sim.people.index_of(id) {
                sim.people[pi].x = x;
                sim.people[pi].y = y;
            }
        }
    }
}

fn step_boat(sim: &mut Settlement, state: &State, i: usize, dt: f64, cfg: &BoatConfig) {
    match sim.boats[i].state {
        BoatState::Moored => {
            sim.boats[i].wait -= dt;
            if sim.boats[i].wait > 0.0 {
                return;
            }
            sim.boats[i].wait = cfg.port_time;
            depart(sim, state, i, cfg);
        }
        BoatState::Outbound | BoatState::Inbound => {
            let arrived = sim.boats[i].sail(dt, cfg.speed);
            feed_crew(sim, state, i);
            if !arrived {
                return;
            }
            if sim.boats[i].state == BoatState::Outbound {
                sim.boats[i].state = BoatState::Trading;
                sim.boats[i].wait = cfg.port_time;
            } else {
                arrive_home(sim, state, i, cfg);
            }
        }
        BoatState::Trading => {
            sim.boats[i].wait -= dt;
            feed_crew(sim, state, i);
            if sim.boats[i].wait > 0.0 {
                return;
            }
            trade_at_port(sim, state, i, cfg);
        }
    }
}

/// Loads whatever the home colony has a surplus of and sets a course for the
/// colony that wants it most. A boat with nothing worth carrying stays tied up.
fn depart(sim: &mut Settlement, state: &State, i: usize, cfg: &BoatConfig) {
    let home_colony = sim.boats[i].colony;
    let ci = match sim.colony_index(home_colony) {
        Some(ci) => ci,
        None => return,
    };
    let dest = match pick_destination(sim, state, i) {
        Some(d) => d,
        None => return,
    };
    let (dest_bi, dest_colony) = dest;
    let moor_from = sim.boats[i].cell();
    let moor_to = match mooring_cell(sim, dest_bi) {
        Some(m) => m,
        None => return,
    };
    let path = match water_path(sim, moor_from, moor_to) {
        Some(p) => p,
        None => return,
    };

    let pop = sim.colony_population(home_colony).max(1);
    let targets = stock_targets(&state.civ.economy, pop);
    let mut loaded = 0.0;
    for res in RES_IDS {
        let spare = sim.colonies[ci].available(res) - targets[res as usize] * 1.15;
        if spare < 2.0 {
            continue;
        }
        let room = cfg.capacity - loaded;
        if room <= 0.0 {
            break;
        }
        let n = spare.floor().min(room);
        if n <= 0.0 {
            continue;
        }
        take_stock(&mut sim.colonies[ci].stock, res, n);
        sim.boats[i].cargo[res as usize] += n;
        loaded += n;
    }
    if loaded < cfg.min_cargo {
        // Put it all back and wait for a better week.
        for res in RES_IDS {
            let n = sim.boats[i].cargo[res as usize];
            if n > 0.0 {
                add_stock(&mut sim.colonies[ci].stock, res, n);
                sim.boats[i].cargo[res as usize] = 0.0;
            }
        }
        return;
    }

    board_crew(sim, state, i, cfg);
    let dest_id = sim.buildings[dest_bi].id;
    let boat = &mut sim.boats[i];
    boat.dest_dock = dest_id;
    boat.dest_colony = dest_colony;
    boat.path = path;
    boat.path_at = 0;
    boat.state = BoatState::Outbound;
    let (name, load) = (boat.name.clone(), boat.load().round());
    let to = sim.colony_name(dest_colony);
    let day = sim.day;
    sim.colonies[ci]
        .econ
        .log_event(format!("{name} sailed for {to} with {load}"), day);
}

/// The reachable dock of another colony whose store is furthest from the cargo
/// this colony has going spare.
fn pick_destination(sim: &mut Settlement, state: &State, i: usize) -> Option<(usize, i32)> {
    let home_colony = sim.boats[i].colony;
    let from = sim.boats[i].cell();
    let mut best: Option<(usize, i32)> = None;
    let mut best_score = 0.35;
    let docks: Vec<usize> = (0..sim.buildings.len())
        .filter(|&bi| {
            sim.buildings[bi].built
                && sim.buildings[bi].def.is_dock
                && sim.buildings[bi].colony != home_colony
        })
        .collect();
    for bi in docks {
        let colony = sim.buildings[bi].colony;
        let ci = match sim.colony_index(colony) {
            // Nobody to trade with in an emptied town, whatever is in its store.
            Some(ci) if sim.is_live(ci) => ci,
            _ => continue,
        };
        let pop = sim.colony_population(colony).max(1);
        let targets = stock_targets(&state.civ.economy, pop);
        // How badly the far colony wants anything at all: the deepest single
        // shortage, because a boat only has to be worth one cargo.
        let mut want: f64 = 0.0;
        for res in RES_IDS {
            let short = (targets[res as usize] - sim.colonies[ci].stock[res as usize])
                / targets[res as usize].max(1.0);
            want = want.max(short);
        }
        let moor = match mooring_cell(sim, bi) {
            Some(m) => m,
            None => continue,
        };
        let d = ((moor.0 - from.0) as f64).hypot((moor.1 - from.1) as f64);
        let score = want - d * 0.004;
        if score <= best_score {
            continue;
        }
        // Only pay for the route search once the port is worth considering.
        if water_path(sim, from, moor).is_none() {
            continue;
        }
        best_score = score;
        best = Some((bi, colony));
    }
    best
}

/// Sells the cargo into the far colony at its own prices, then spends the take
/// on whatever home is short of and this port has going spare.
fn trade_at_port(sim: &mut Settlement, state: &State, i: usize, cfg: &BoatConfig) {
    let dest_colony = sim.boats[i].dest_colony;
    let home_colony = sim.boats[i].colony;
    let (Some(di), Some(hi)) = (sim.colony_index(dest_colony), sim.colony_index(home_colony)) else {
        turn_for_home(sim, i);
        return;
    };

    let mut sold: f64 = 0.0;
    for res in RES_IDS {
        let n = sim.boats[i].cargo[res as usize];
        if n <= 0.0 {
            continue;
        }
        let unit = sim.colonies[di].econ.price_of(res) * (1.0 - cfg.margin);
        let paid = (n * unit).min(sim.colonies[di].econ.coin.max(0.0));
        let taken = if unit > 0.0 { (paid / unit).floor().min(n) } else { n };
        if taken <= 0.0 {
            continue;
        }
        add_stock(&mut sim.colonies[di].stock, res, taken);
        sim.colonies[di].econ.coin -= taken * unit;
        sim.colonies[di].econ.trade_balance -= taken * unit;
        sim.boats[i].cargo[res as usize] -= taken;
        sim.boats[i].coin += taken * unit;
        sold += taken;
    }

    let pop = sim.colony_population(home_colony).max(1);
    let home_targets = stock_targets(&state.civ.economy, pop);
    let dest_pop = sim.colony_population(dest_colony).max(1);
    let dest_targets = stock_targets(&state.civ.economy, dest_pop);
    let mut bought: f64 = 0.0;
    let mut room = cfg.capacity - sim.boats[i].load();
    // Shortages at home, deepest first, so a boat comes back with what was
    // actually missing rather than with whatever was nearest the gangplank.
    let mut wants: Vec<(Res, f64)> = RES_IDS
        .iter()
        .map(|&res| {
            let short = (home_targets[res as usize] - sim.colonies[hi].stock[res as usize])
                / home_targets[res as usize].max(1.0);
            (res, short)
        })
        .filter(|&(_, short)| short > 0.15)
        .collect();
    wants.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (res, _) in wants {
        if room <= 0.0 || sim.boats[i].coin <= 0.0 {
            break;
        }
        let spare = sim.colonies[di].available(res) - dest_targets[res as usize] * 0.9;
        if spare < 1.0 {
            continue;
        }
        let unit = sim.colonies[di].econ.price_of(res) * (1.0 + cfg.margin);
        let affordable = (sim.boats[i].coin / unit.max(0.01)).floor();
        let n = spare.floor().min(room).min(affordable);
        if n <= 0.0 {
            continue;
        }
        take_stock(&mut sim.colonies[di].stock, res, n);
        sim.colonies[di].econ.coin += n * unit;
        sim.colonies[di].econ.trade_balance += n * unit;
        sim.boats[i].cargo[res as usize] += n;
        sim.boats[i].coin -= n * unit;
        room -= n;
        bought += n;
    }

    let day = sim.day;
    let name = sim.boats[i].name.clone();
    let home_name = sim.colony_name(home_colony);
    sim.colonies[di].econ.log_event(
        format!(
            "{name} of {home_name} landed {} and took on {}",
            sold.round(),
            bought.round()
        ),
        day,
    );
    turn_for_home(sim, i);
}

fn turn_for_home(sim: &mut Settlement, i: usize) {
    let home_dock = sim.boats[i].home_dock;
    let from = sim.boats[i].cell();
    let moor = sim
        .building_index(home_dock)
        .and_then(|bi| mooring_cell(sim, bi));
    let path = match moor.and_then(|m| water_path(sim, from, m)) {
        Some(p) => p,
        None => {
            // The way home has silted up. The boat and its cargo are written
            // off where they stand rather than sailing through dry land.
            sim.boats[i].state = BoatState::Moored;
            sim.boats[i].wait = 60.0;
            return;
        }
    };
    let boat = &mut sim.boats[i];
    boat.path = path;
    boat.path_at = 0;
    boat.state = BoatState::Inbound;
}

fn arrive_home(sim: &mut Settlement, state: &State, i: usize, cfg: &BoatConfig) {
    let colony = sim.boats[i].colony;
    let ci = match sim.colony_index(colony) {
        Some(ci) => ci,
        None => return,
    };
    let mut landed = 0.0;
    for res in RES_IDS {
        let n = sim.boats[i].cargo[res as usize];
        if n <= 0.0 {
            continue;
        }
        let at = sim.boats[i].cell();
        sim.boats[i].cargo[res as usize] = 0.0;
        landed += sim.deposit(state, ci, res, n, Some(at));
    }
    let coin = sim.boats[i].coin;
    sim.boats[i].coin = 0.0;
    sim.colonies[ci].econ.coin += coin;
    sim.colonies[ci].econ.trade_balance += coin;
    sim.colonies[ci].econ.trades += 1;
    sim.boats[i].state = BoatState::Moored;
    sim.boats[i].wait = cfg.port_time;
    sim.boats[i].voyages += 1;
    sim.boats[i].dest_dock = 0;
    sim.boats[i].dest_colony = 0;
    let day = sim.day;
    let name = sim.boats[i].name.clone();
    sim.colonies[ci].econ.log_event(
        format!("{name} came home with {} and {} coin", landed.round(), coin.round()),
        day,
    );
    land_crew(sim, i);
}

/// Dock workers step aboard for the voyage. While aboard they are not drawn
/// and take no tasks; the boat feeds them out of its own hold.
fn board_crew(sim: &mut Settlement, state: &State, i: usize, cfg: &BoatConfig) {
    let dock_id = sim.boats[i].home_dock;
    let boat_id = sim.boats[i].id;
    let workers = match sim.building_index(dock_id) {
        Some(bi) => sim.buildings[bi].workers.clone(),
        None => return,
    };
    let mut taken = 0;
    for id in workers {
        if taken >= cfg.crew {
            break;
        }
        let pi = match sim.people.index_of(id) {
            Some(pi) => pi,
            None => continue,
        };
        if !sim.people[pi].alive || sim.people[pi].aboard != 0 {
            continue;
        }
        crate::civ::tasks::abandon_task(sim, pi);
        sim.people[pi].step_outside();
        sim.people[pi].aboard = boat_id;
        sim.people[pi].profession = crate::civ::people::Profession::Sailor;
        let day = sim.day;
        let name = sim.boats[i].name.clone();
        sim.people[pi].log(day, format!("shipped out on the {name}"));
        sim.boats[i].crew.push(id);
        taken += 1;
    }
    // A meal a head for the passage, out of the cargo if there is any to spare.
    let need = (taken as f64) * state.civ.people.meal_size * 2.0;
    if need > 0.0 {
        if let Some(ci) = sim.colony_index(sim.boats[i].colony) {
            let got = take_stock(&mut sim.colonies[ci].stock, Res::Food, need);
            sim.boats[i].cargo[Res::Food as usize] += got;
        }
    }
}

fn land_crew(sim: &mut Settlement, i: usize) {
    let crew = std::mem::take(&mut sim.boats[i].crew);
    let (x, y) = (sim.boats[i].x, sim.boats[i].y);
    for id in crew {
        let pi = match sim.people.index_of(id) {
            Some(pi) => pi,
            None => continue,
        };
        sim.people[pi].aboard = 0;
        // Set down on the nearest dry cell so nobody wakes up in the river.
        let spot = sim
            .free_cell_near(x.floor() as i32, y.floor() as i32)
            .unwrap_or((x.floor() as i32, y.floor() as i32));
        sim.people[pi].x = spot.0 as f64 + 0.5;
        sim.people[pi].y = spot.1 as f64 + 0.5;
        sim.people[pi].clear_task();
    }
}

fn feed_crew(sim: &mut Settlement, state: &State, i: usize) {
    if sim.boats[i].cargo[Res::Food as usize] <= 0.0 {
        return;
    }
    let meal = state.civ.people.meal_size;
    let eat_at = state.civ.people.eat_at;
    let crew = sim.boats[i].crew.clone();
    for id in crew {
        let pi = match sim.people.index_of(id) {
            Some(pi) => pi,
            None => continue,
        };
        if sim.people[pi].hunger < eat_at {
            continue;
        }
        let got = take_stock(&mut sim.boats[i].cargo, Res::Food, meal);
        if got > 0.0 {
            sim.people[pi].eat(got);
        }
    }
}

/// Where a boat sits on screen, in world pixels, with the roll of the water in
/// it.
pub fn boat_anchor(sim: &Settlement, boat: &Boat) -> (i32, i32) {
    let world = sim.world();
    let x = (boat.x * world.cell_px as f64).round() as i32;
    let lift = clamp((boat.bob * 2.2).sin() * 0.9, -1.0, 1.0);
    let y = (world.sky_px as f64 + boat.y * world.depth_px as f64 + lift).round() as i32;
    (x, y)
}
