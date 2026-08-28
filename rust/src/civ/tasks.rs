//! What a settler does with the next second of their life.
//!
//! Every task is a small state machine with a phase and a target: walk there,
//! work, carry the result somewhere. Nothing here reads a global; the
//! settlement is passed in, which keeps the decision making testable and keeps
//! the settlement itself down to world, buildings and books.
//!
//! The one rule that shapes all of it: material only moves because a person
//! carried it. A wall goes up because somebody walked wood to the site.
//!
//! A settler belongs to a colony, and every question about stock, wages and
//! what is worth doing is asked of that colony rather than of the map. Two
//! towns on one map therefore make different decisions on the same tick.

use serde::{Deserialize, Serialize};

use crate::civ::buildings::{Job, BUILDINGS};
use crate::civ::economy::{buy_food, pay_wage, stock_targets};
use crate::civ::harvest::{lore_patience, lore_weight};
use crate::civ::people::{carry_limit, is_work_time, Profession};
use crate::civ::resources::{take_stock, Res};
use crate::civ::settlement::Settlement;
use crate::species::SizeClass;
use crate::state::State;
use crate::util::{clamp01, clampi};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Approach,
    Working,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HaulTarget {
    Site,
    Input,
    Output,
    /// A stall counter. Reaches the same bench as `Input`, but the person
    /// carrying it pays the town for what they are carrying.
    Stall,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Task {
    Idle {
        timer: f64,
    },
    Sleep {
        building_id: i32,
        phase: Phase,
        /// A room paid for rather than a bed at home.
        hired: bool,
    },
    Eat {
        to_id: i32,
    },
    Harvest {
        plant_id: i32,
        /// Where the plant was last seen in the plant list, so the lookup is a
        /// bounds check rather than a scan of every plant on the map.
        hint: usize,
        #[serde(with = "yields")]
        yields: &'static [(Res, f64)],
        regrow: f64,
        phase: Phase,
        timer: f64,
    },
    Mine {
        deposit_id: i32,
        #[serde(with = "yields")]
        yields: &'static [(Res, f64)],
        phase: Phase,
        timer: f64,
    },
    Pickup {
        pile_id: i32,
    },
    Deliver {
        to_id: i32,
    },
    Haul {
        res: Res,
        amount: f64,
        from_id: i32,
        to_id: i32,
        target: HaulTarget,
        phase: Phase,
    },
    Build {
        building_id: i32,
        phase: Phase,
    },
    Station {
        building_id: i32,
        phase: Phase,
        timer: f64,
    },
    /// A bucket of water for a farm that has run dry. The walk out is to the
    /// nearest bank, the walk back is to the field.
    Water {
        building_id: i32,
        /// Whether the bucket is full, which is what decides which way they
        /// are walking.
        full: bool,
        phase: Phase,
    },
    /// Buying something over a counter, with this settler's own coin, from
    /// another settler.
    Shop {
        building_id: i32,
        /// The one ware they walked over for, or none: a browser buys
        /// whatever takes their eye.
        want: Option<Res>,
    },
}

impl Task {
    /// True while the settler is doing the work rather than walking to it.
    /// Only the tasks that have somewhere to stand and something to do there
    /// answer yes; an errand is over the moment the walk ends.
    pub fn working(&self) -> bool {
        match self {
            Task::Harvest { phase, .. }
            | Task::Mine { phase, .. }
            | Task::Build { phase, .. }
            | Task::Station { phase, .. }
            | Task::Water { phase, .. }
            | Task::Haul { phase, .. } => *phase == Phase::Working,
            _ => false,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Task::Idle { .. } => "idle",
            Task::Sleep { hired: true, .. } => "at an inn",
            Task::Sleep { .. } => "sleep",
            Task::Eat { .. } => "eat",
            Task::Harvest { .. } => "harvest",
            Task::Mine { .. } => "mine",
            Task::Pickup { .. } => "pickup",
            Task::Deliver { .. } => "deliver",
            Task::Haul { .. } => "haul",
            Task::Build { .. } => "build",
            Task::Station { .. } => "station",
            Task::Water { .. } => "water",
            Task::Shop { .. } => "shopping",
        }
    }

    pub fn is_sleep(&self) -> bool {
        matches!(self, Task::Sleep { .. })
    }

    /// A meal bought at a stall is still a meal, so the hunger loop has to
    /// count it or a settler on their way to the counter is sent to the store
    /// on the very next tick.
    pub fn is_eat(&self) -> bool {
        matches!(self, Task::Eat { .. })
            || matches!(self, Task::Shop { want: Some(res), .. } if *res == Res::Food)
    }

    /// Whether the task would be left dangling by removing this building.
    pub fn touches_building(&self, id: i32) -> bool {
        match self {
            Task::Sleep { building_id, .. }
            | Task::Build { building_id, .. }
            | Task::Station { building_id, .. }
            | Task::Shop { building_id, .. } => *building_id == id,
            Task::Eat { to_id } | Task::Deliver { to_id } => *to_id == id,
            Task::Haul { from_id, to_id, .. } => *from_id == id || *to_id == id,
            _ => false,
        }
    }

    /// Work counts as work: sleeping and standing about do not.
    fn is_effort(&self) -> bool {
        !matches!(self, Task::Sleep { .. } | Task::Idle { .. })
    }
}

/// Foraging without a hut: anyone can strip the low growth for something to
/// eat, which is what keeps a colony alive before it has built anything.
const WILD_CLASSES: &[SizeClass] = &[SizeClass::Ground, SizeClass::Herb, SizeClass::Vine];
const WILD_YIELDS: &[(Res, f64)] = &[(Res::Food, 1.0), (Res::Fiber, 0.3)];

#[derive(Clone, Copy)]
struct HarvestJob {
    radius: f64,
    classes: &'static [SizeClass],
    yields: &'static [(Res, f64)],
    regrow: f64,
}

const WILD_JOB: HarvestJob = HarvestJob {
    radius: 30.0,
    classes: WILD_CLASSES,
    yields: WILD_YIELDS,
    regrow: 0.35,
};

/// The yield table a task pays out from, through a save and back.
///
/// A task holds its table by reference because the table always came from a
/// building definition or from the wild forage list. One read back off a file
/// is matched against those, so the reference points at the table the rest of
/// the program is using; a list that matches nothing is kept as it was
/// written rather than dropping the task.
mod yields {
    use super::{table_like, Res};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        table: &&'static [(Res, f64)],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.collect_seq(table.iter())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<&'static [(Res, f64)], D::Error> {
        Ok(table_like(&Vec::<(Res, f64)>::deserialize(d)?))
    }
}

fn table_like(want: &[(Res, f64)]) -> &'static [(Res, f64)] {
    let same = |table: &[(Res, f64)]| {
        table.len() == want.len()
            && table.iter().zip(want).all(|(a, b)| a.0 == b.0 && a.1 == b.1)
    };
    if same(WILD_YIELDS) {
        return WILD_YIELDS;
    }
    for def in BUILDINGS {
        if let Some(job) = def.job {
            let table = job.produces();
            if same(table) {
                return table;
            }
        }
    }
    Box::leak(want.to_vec().into_boxed_slice())
}

/// Being out after dark with no lamp in sight wears on somebody. Daylight, a
/// roof and a lit street all settle it again; what is left is what decides
/// whether they would rather spend their coin on a lamp post than keep it.
fn tick_fear(sim: &mut Settlement, state: &State, pi: usize, dt: f64) {
    let pcfg = &state.civ.people;
    let dark = sim.daylight(state) < 0.35;
    let exposed = dark && !sim.people[pi].indoors() && {
        let (c, r) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
        !sim.lit_at(c, r)
    };
    let p = &mut sim.people[pi];
    // A hardy settler takes longer to be worn down: the same night is not the
    // same night to everybody.
    let nerve = 1.0 - p.traits.hardiness * 0.6;
    p.fear = clamp01(if exposed {
        p.fear + pcfg.fear_gain * nerve * dt
    } else {
        p.fear - pcfg.fear_ease * dt
    });
}

pub fn update_person(sim: &mut Settlement, state: &State, pi: usize, dt: f64) {
    let pcfg = &state.civ.people;
    {
        let p = &mut sim.people[pi];
        p.age += (dt / pcfg.day_length) * pcfg.years_per_day;
        p.adult_age = pcfg.adult_age;
        let working = p.task.as_ref().is_some_and(|t| t.is_effort());
        p.tick_needs(dt, pcfg, working);

        if p.age > p.lifespan {
            p.alive = false;
            p.cause = Some("old age".to_string());
            return;
        }
        if p.health <= 0.0 {
            p.alive = false;
            p.cause = Some("hunger".to_string());
            return;
        }
    }

    // What the dark is doing to them. Read before anything else moves, so it
    // is about where they have been standing rather than where they end up.
    tick_fear(sim, state, pi, dt);

    // Anyone at sea is the boat's problem until it ties up again.
    if sim.people[pi].aboard != 0 {
        return;
    }

    // Somebody picked up off the map keeps aging and getting hungry, since
    // being carried about is not a way out of either, but does nothing else
    // until they are put down.
    if sim.held != 0 && sim.held == sim.people[pi].id {
        return;
    }

    let health = sim.people[pi].health;
    let hardy = sim.people[pi].traits.hardiness;
    let sick = pcfg.sickness_rate * dt / pcfg.day_length.max(1.0)
        * (1.0 - sim.well_coverage(pi) * 0.6)
        * (1.3 - hardy * 0.6);
    if sick > 0.0 && sim.rng.chance(sick * (1.6 - health)) {
        sim.people[pi].alive = false;
        sim.people[pi].cause = Some("sickness".to_string());
        return;
    }

    // Children eat from the same store the adults fill; they simply do not work
    // for it. Without this they starve in their first days.
    sim.people[pi].eat_cooldown = (sim.people[pi].eat_cooldown - dt).max(0.0);
    sim.people[pi].shop_cooldown = (sim.people[pi].shop_cooldown - dt).max(0.0);
    if !sim.people[pi].adult() {
        if hungry_enough(sim, state, pi) {
            abandon_task(sim, pi);
            if !start_eat(sim, pi) {
                sim.people[pi].eat_cooldown = 4.0;
            }
        }
        if sim.people[pi].task.as_ref().is_some_and(|t| t.is_eat()) {
            run_task(sim, state, pi, dt);
            return;
        }
        child_behavior(sim, state, pi, dt);
        return;
    }

    let night = !is_work_time(sim.time, pcfg);
    if sim.people[pi].sleeping {
        if !night || sim.people[pi].energy >= 1.0 {
            sim.people[pi].sleeping = false;
            sim.leave_building(pi);
            sim.people[pi].clear_task();
        } else {
            return;
        }
    }
    if hungry_enough(sim, state, pi) {
        abandon_task(sim, pi);
        // With nothing in the store, going hungry is not a reason to stand
        // still: the person falls through to work, which for a hungry colony
        // means somebody goes out looking for food. The cooldown keeps a failed
        // attempt from cancelling that work on the very next tick.
        if !start_eat(sim, pi) {
            sim.people[pi].eat_cooldown = 4.0;
        }
    } else if night
        && sim.people[pi]
            .task
            .as_ref()
            .is_none_or(|t| !t.is_sleep() && !t.is_eat())
    {
        abandon_task(sim, pi);
        start_sleep(sim, state, pi);
    }
    if sim.people[pi].task.is_none() {
        choose_task(sim, state, pi);
    }
    if sim.people[pi].task.is_some() {
        run_task(sim, state, pi, dt);
    }
}

fn hungry_enough(sim: &Settlement, state: &State, pi: usize) -> bool {
    let p = &sim.people[pi];
    p.hunger > state.civ.people.eat_at
        && p.eat_cooldown == 0.0
        && !p.task.as_ref().is_some_and(|t| t.is_eat())
}

pub fn child_behavior(sim: &mut Settlement, state: &State, pi: usize, dt: f64) {
    let idle = matches!(sim.people[pi].task, Some(Task::Idle { .. }));
    if !idle {
        sim.leave_building(pi);
        let home = sim.people[pi].home;
        let anchor = match sim.building_index(home) {
            Some(bi) => sim.access_cell(bi),
            None => (sim.people[pi].cell_col(), sim.people[pi].cell_row()),
        };
        let c = clampi(anchor.0 + sim.rng.int(-3, 3), 0, sim.world().cols - 1);
        let r = clampi(anchor.1 + sim.rng.int(-3, 3), 0, sim.world().rows - 1);
        let timer = sim.rng.range(1.0, 4.0);
        sim.people[pi].task = Some(Task::Idle { timer });
        if sim.walkable(c, r) {
            let (sc, sr) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
            if let Some(path) = sim.find_path(sc, sr, c, r) {
                sim.people[pi].set_path(path);
            }
        }
    }
    if walk(sim, state, pi, dt, 0.7) {
        if let Some(Task::Idle { timer }) = &mut sim.people[pi].task {
            *timer -= dt;
            if *timer <= 0.0 {
                sim.people[pi].clear_task();
            }
        }
    }
}

pub fn walk(sim: &mut Settlement, state: &State, pi: usize, dt: f64, speed_scale: f64) -> bool {
    let pcfg = &state.civ.people;
    let (cc, cr) = {
        let p = &sim.people[pi];
        (
            clampi(p.cell_col(), 0, sim.world().cols - 1),
            clampi(p.cell_row(), 0, sim.world().rows - 1),
        )
    };
    let road = sim.traffic[sim.idx(cc, cr)] as f64;
    let swimming = sim.in_water(cc, cr);
    let p = &sim.people[pi];
    // Swimming is slower than walking and gets no help from a worn path, there
    // being no path in the water to have worn.
    let terrain = if swimming {
        pcfg.swim_speed.clamp(0.05, 1.0)
    } else {
        1.0 + clamp01(road / 6.0) * pcfg.road_speed_bonus
    };
    let speed = pcfg.walk_speed
        * speed_scale
        * terrain
        * (0.7 + p.energy * 0.3)
        * (0.6 + p.health * 0.4);
    let before = !sim.people[pi].path.is_empty();
    let done = sim.people[pi].move_along(dt, speed);
    if before {
        let (cc, cr) = {
            let p = &sim.people[pi];
            (
                clampi(p.cell_col(), 0, sim.world().cols - 1),
                clampi(p.cell_row(), 0, sim.world().rows - 1),
            )
        };
        // Nothing wears into water, so a crossing never becomes a road.
        if !sim.in_water(cc, cr) {
            let i = sim.idx(cc, cr);
            sim.traffic[i] = (sim.traffic[i] + (dt * 2.0) as f32).min(20.0);
        }
    }
    done
}

/// Walks to a building, trying its other free sides when the first one cannot
/// be reached. A single unreachable spot used to be enough to starve somebody
/// standing next to a full store.
pub fn path_to_building(sim: &mut Settlement, pi: usize, bi: usize) -> bool {
    let cells = sim.access_cells(bi);
    for cell in cells.iter().take(4) {
        if path_to(sim, pi, cell.0, cell.1) {
            return true;
        }
    }
    false
}

pub fn path_to(sim: &mut Settlement, pi: usize, col: i32, row: i32) -> bool {
    sim.leave_building(pi);
    let (sc, sr) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    match sim.find_path(sc, sr, col, row) {
        Some(path) => {
            sim.people[pi].set_path(path);
            true
        }
        None => false,
    }
}

pub fn abandon_task(sim: &mut Settlement, pi: usize) {
    let task = match sim.people[pi].task.clone() {
        Some(t) => t,
        None => return,
    };
    let ci = sim.colony_of(pi);
    match task {
        Task::Haul { res, amount, to_id, target, phase, .. } => {
            if phase == Phase::Approach {
                sim.release_stock(ci, res, amount);
            }
            if let Some(di) = sim.building_index(to_id) {
                let dest = &mut sim.buildings[di];
                let i = res as usize;
                match target {
                    HaulTarget::Site => dest.incoming[i] = (dest.incoming[i] - amount).max(0.0),
                    HaulTarget::Input | HaulTarget::Stall => {
                        dest.reserved_in[i] = (dest.reserved_in[i] - amount).max(0.0)
                    }
                    HaulTarget::Output => {
                        dest.reserved_out[i] = (dest.reserved_out[i] - amount).max(0.0)
                    }
                }
            }
        }
        Task::Harvest { plant_id, hint, .. } => {
            let person_id = sim.people[pi].id;
            if let Some(index) = sim.plant_sim.plant_at(plant_id, hint) {
                if sim.plant_sim.plants[index].claimed_by == person_id {
                    sim.plant_sim.plants[index].claimed_by = 0;
                    sim.claim_plant(plant_id, 0);
                }
            }
        }
        Task::Pickup { pile_id } => {
            let person_id = sim.people[pi].id;
            if let Some(index) = sim.pile_index(pile_id) {
                if sim.piles[index].claimed_by == person_id {
                    sim.piles[index].claimed_by = 0;
                }
            }
        }
        Task::Build { building_id, .. } => {
            if let Some(bi) = sim.building_index(building_id) {
                sim.buildings[bi].builders = (sim.buildings[bi].builders - 1).max(0);
            }
        }
        // Only a room that was paid for is given up; a bed at home is still
        // theirs whatever they walk off to do.
        Task::Sleep { building_id, hired: true, .. } => {
            if let Some(bi) = sim.building_index(building_id) {
                let id = sim.people[pi].id;
                sim.buildings[bi].guests.retain(|&g| g != id);
            }
        }
        _ => {}
    }
    sim.people[pi].clear_task();
}

/// Tries every store of the settler's own colony, nearest first: an
/// unreachable one is not a reason to go hungry while another has food.
pub fn start_eat(sim: &mut Settlement, pi: usize) -> bool {
    let ci = sim.colony_of(pi);
    // A counter close at hand beats a walk to the store, and the coin goes to
    // a neighbor rather than into the treasury. This is most of what makes a
    // stall worth keeping.
    if sim.people[pi].coin > 0.0 {
        if let Some(bi) = sim.stall_near(pi, Some(Res::Food)) {
            if path_to_building(sim, pi, bi) {
                let id = sim.buildings[bi].id;
                sim.people[pi].task = Some(Task::Shop { building_id: id, want: Some(Res::Food) });
                return true;
            }
        }
    }
    if sim.colonies[ci].stock[Res::Food as usize] < 1.0 {
        return false;
    }
    let colony = sim.colonies[ci].id;
    let (px, py) = (sim.people[pi].x, sim.people[pi].y);
    let mut stores: Vec<usize> = (0..sim.buildings.len())
        .filter(|&i| {
            sim.buildings[i].built && sim.buildings[i].def.is_store && sim.buildings[i].colony == colony
        })
        .collect();
    stores.sort_by(|&a, &z| {
        let da = (sim.buildings[a].col as f64 - px).powi(2) + (sim.buildings[a].row as f64 - py).powi(2);
        let dz = (sim.buildings[z].col as f64 - px).powi(2) + (sim.buildings[z].row as f64 - py).powi(2);
        da.partial_cmp(&dz).unwrap_or(std::cmp::Ordering::Equal)
    });
    for bi in stores {
        if !path_to_building(sim, pi, bi) {
            continue;
        }
        let id = sim.buildings[bi].id;
        sim.people[pi].task = Some(Task::Eat { to_id: id });
        return true;
    }
    false
}

/// Home first. Somebody with no roof of their own takes a room at an inn if
/// there is one with a bed free and they have the coin; failing both they
/// sleep where they stand.
pub fn start_sleep(sim: &mut Settlement, state: &State, pi: usize) {
    let home = sim.people[pi].home;
    let at_home = sim
        .building_index(home)
        .filter(|&bi| sim.buildings[bi].built && !sim.buildings[bi].upgrading);
    if let Some(bi) = at_home {
        let id = sim.buildings[bi].id;
        sim.people[pi].task = Some(Task::Sleep { building_id: id, phase: Phase::Approach, hired: false });
        if !path_to_building(sim, pi, bi) {
            sleep_rough(sim, pi);
        }
        return;
    }

    let ci = sim.colony_of(pi);
    let colony = sim.colonies[ci].id;
    let price = state.civ.people.inn_price;
    let (cc, cr) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    if sim.people[pi].coin >= price {
        if let Some(bi) = sim.inn_near(colony, cc, cr) {
            if path_to_building(sim, pi, bi) {
                let id = sim.buildings[bi].id;
                let person_id = sim.people[pi].id;
                sim.buildings[bi].guests.push(person_id);
                sim.people[pi].task =
                    Some(Task::Sleep { building_id: id, phase: Phase::Approach, hired: true });
                return;
            }
        }
    }
    sleep_rough(sim, pi);
}

/// Nobody with a roof and nothing at an inn still walks home. Lying down where
/// the day happened to end leaves a forager asleep in the woods a morning's
/// walk from anything, so they go back to the middle of their town and sleep
/// among everybody else. Only somebody who cannot get there at all - cut off,
/// or already standing in it - sleeps where they are.
fn sleep_rough(sim: &mut Settlement, pi: usize) {
    sim.leave_building(pi);
    let ci = sim.colony_of(pi);
    let (tc, tr) = sim.colonies[ci].center;
    let (cc, cr) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    let near = (cc - tc).abs() <= 1 && (cr - tr).abs() <= 1;
    if !near {
        if let Some(spot) = sim.free_spot_near(tc, tr) {
            if let Some(path) = sim.find_path(cc, cr, spot.0, spot.1) {
                sim.people[pi].path = path;
                sim.people[pi].path_at = 0;
                sim.people[pi].task =
                    Some(Task::Sleep { building_id: 0, phase: Phase::Approach, hired: false });
                // Still a night in the open, and still worth complaining about;
                // the walk back is about where they wake up, not comfort.
                sim.people[pi].happiness = clamp01(sim.people[pi].happiness - 0.02);
                return;
            }
        }
    }
    sim.people[pi].sleeping = true;
    sim.people[pi].task = Some(Task::Sleep { building_id: 0, phase: Phase::Working, hired: false });
    // A night in the open is worth complaining about.
    sim.people[pi].happiness = clamp01(sim.people[pi].happiness - 0.02);
}

pub fn choose_task(sim: &mut Settlement, state: &State, pi: usize) {
    let ci = sim.colony_of(pi);
    if sim.people[pi].carrying() {
        start_deliver(sim, state, pi);
        return;
    }
    // A colony that is running out of food puts everyone on food, whatever
    // their trade, until the store is off the floor again.
    let colony = sim.colonies[ci].id;
    let pop = sim.colony_population(colony).max(1) as f64;
    let food_short =
        sim.colonies[ci].stock[Res::Food as usize] < pop * state.civ.people.meal_size * 2.0;
    let work = sim
        .building_index(sim.people[pi].work)
        .filter(|&bi| sim.buildings[bi].built);
    let food_work = work.is_some_and(|bi| {
        sim.buildings[bi]
            .def
            .job
            .as_ref()
            .is_some_and(|job| job.produces().iter().any(|&(r, _)| r == Res::Food))
    });
    // A load cut by hand was asked for, and outranks whatever this settler
    // would have picked for themselves - their own trade included.
    if start_gleaning(sim, state, pi, food_short) {
        return;
    }
    if food_short && !food_work && start_forage(sim, state, pi) {
        return;
    }
    if let Some(bi) = work {
        if let Some(job) = sim.buildings[bi].def.job {
            match job {
                Job::Harvest { classes, yields, regrow } => {
                    let radius = if sim.buildings[bi].def.radius > 0.0 {
                        sim.buildings[bi].def.radius
                    } else {
                        12.0
                    };
                    let hjob = HarvestJob { radius, classes, yields, regrow };
                    if start_harvest(sim, state, pi, Some(bi), hjob) {
                        return;
                    }
                }
                Job::Mine { .. } => {
                    if start_mine(sim, pi, bi) {
                        return;
                    }
                }
                Job::Farm { .. } => {
                    // A field too dry to be worth working is worth carrying to
                    // first, unless there is no water within reach of the town.
                    if sim.buildings[bi].water < state.civ.work.farm_thirsty
                        && start_water(sim, state, pi, bi)
                    {
                        return;
                    }
                    start_station(sim, pi, bi);
                    return;
                }
                Job::Research | Job::Trade | Job::Innkeep | Job::Ferry => {
                    start_station(sim, pi, bi);
                    return;
                }
                // A keeper stocks their own counter before they stand behind
                // it: nothing else in the town will do it for them.
                Job::Sell => {
                    if start_restock(sim, state, pi, bi) {
                        return;
                    }
                    start_station(sim, pi, bi);
                    return;
                }
                Job::Craft { .. } => {
                    if sim.craft_ready(bi) {
                        start_station(sim, pi, bi);
                        return;
                    }
                }
            }
        }
    }
    if start_labor(sim, state, pi) {
        return;
    }
    // Nothing queued: forage if the store is thin on food, otherwise wander.
    if sim.colonies[ci].stock[Res::Food as usize] < pop * state.civ.people.meal_size
        && start_forage(sim, state, pi)
    {
        return;
    }
    if start_browse(sim, state, pi) {
        return;
    }
    start_wander(sim, pi);
}

/// A settler with coin to spare and nothing to do goes and looks at what is on
/// the counters. This is the only thing that moves coin between two settlers
/// without the treasury in the middle.
pub fn start_browse(sim: &mut Settlement, state: &State, pi: usize) -> bool {
    if !state.civ.build.stalls || sim.people[pi].shop_cooldown > 0.0 || !sim.people[pi].adult() {
        return false;
    }
    // Spare coin only. Nobody spends the price of a roof on a bolt of cloth,
    // and a thrifty settler holds out longer than a spendthrift.
    let p = &sim.people[pi];
    let threshold = 8.0 + p.traits.thrift * 40.0;
    if p.coin < threshold {
        return false;
    }
    let bi = match sim.stall_near(pi, None) {
        Some(bi) => bi,
        None => return false,
    };
    if !path_to_building(sim, pi, bi) {
        return false;
    }
    let id = sim.buildings[bi].id;
    sim.people[pi].task = Some(Task::Shop { building_id: id, want: None });
    true
}

/// A keeper buys their own stock, with their own coin, out of the town store.
///
/// Only what the town has spare: a keeper who cleared the granary in a famine
/// would be selling the town its own last meal back to it.
fn start_restock(sim: &mut Settlement, state: &State, pi: usize, bi: usize) -> bool {
    let ci = sim.colony_of(pi);
    let colony = sim.colonies[ci].id;
    if sim.buildings[bi].owner != sim.people[pi].id || sim.people[pi].carrying() {
        return false;
    }
    let cap = carry_limit(&state.civ.people, &sim.mods_of(ci));
    let want = (cap * 0.5).ceil().max(2.0);
    let targets = stock_targets(&state.civ.economy, sim.colony_population(colony));
    let mut pick: Option<(Res, f64)> = None;
    for &res in sim.buildings[bi].def.sells {
        let counter =
            sim.buildings[bi].inv[res as usize] + sim.buildings[bi].reserved_in[res as usize];
        if counter >= want {
            continue;
        }
        let spare = (sim.available_stock(ci, res) - targets[res as usize] * 0.5).max(0.0);
        if spare < 1.0 {
            continue;
        }
        let price = sim.colonies[ci].econ.price_of(res).max(0.01);
        let afford = (sim.people[pi].coin / price).floor();
        let take = (want - counter).min(spare).min(cap).min(afford).floor();
        if take >= 1.0 {
            pick = Some((res, take));
            break;
        }
    }
    let (res, amount) = match pick {
        Some(p) => p,
        None => return false,
    };
    let (cc, cr) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    let store = match sim.nearest_store(colony, cc, cr) {
        Some(s) => s,
        None => return false,
    };
    if !path_to_building(sim, pi, store) {
        return false;
    }
    sim.reserve_stock(ci, res, amount);
    sim.buildings[bi].reserved_in[res as usize] += amount;
    let from_id = sim.buildings[store].id;
    let to_id = sim.buildings[bi].id;
    sim.people[pi].task = Some(Task::Haul {
        res,
        amount,
        from_id,
        to_id,
        target: HaulTarget::Stall,
        phase: Phase::Approach,
    });
    true
}

pub fn start_forage(sim: &mut Settlement, state: &State, pi: usize) -> bool {
    start_harvest(sim, state, pi, None, WILD_JOB)
}

/// How much further somebody will go for a load that was cut by hand than for
/// one that merely fell somewhere: this one was pointed at.
const ASKED_REACH: f64 = 1.5;

/// What the nearest load beyond anybody's reach is worth as a job. Below every
/// other option there is, so it is only ever taken when there is nothing
/// nearer to be doing.
const FAR_PILE_SCORE: f64 = 2.0;

/// How far somebody will go for a load, in cells, and half again as far for
/// one that was asked for.
fn fetch_reach(state: &State, by_hand: bool) -> f64 {
    let reach = state.civ.work.fetch_reach.max(1.0);
    if by_hand {
        reach * ASKED_REACH
    } else {
        reach
    }
}

/// Fetching what the pointer cut. This is the whole of what the hand tool does
/// to the town's plans: it puts a load on the ground and marks it as asked for,
/// and the next settler to make a decision goes and gets it.
fn start_gleaning(sim: &mut Settlement, state: &State, pi: usize, food_short: bool) -> bool {
    let ci = sim.colony_of(pi);
    let person_id = sim.people[pi].id;
    let (px, py) = (sim.people[pi].x, sim.people[pi].y);
    let mut best: Option<(f64, i32)> = None;
    for pile in &sim.piles {
        if !pile.by_hand || (pile.claimed_by != 0 && pile.claimed_by != person_id) {
            continue;
        }
        // A town running out of food fetches food and nothing else, however
        // loudly the pointer asked for the rest of it.
        if food_short && pile.res != Res::Food {
            continue;
        }
        if !sim.wanted(state, ci, pile.res) {
            continue;
        }
        let d = (pile.col as f64 - px).hypot(pile.row as f64 - py);
        // A hard limit here, with no falling back on the nearest: this is the
        // path that jumps the queue, and a load on the far side of the map is
        // not a reason to drop what somebody was about to do. It is still
        // offered as ordinary work below.
        if d > fetch_reach(state, true) {
            continue;
        }
        match best {
            Some((near, _)) if near <= d => {}
            _ => best = Some((d, pile.id)),
        }
    }
    let pile_id = match best {
        Some((_, id)) => id,
        None => return false,
    };
    take_labor_task(sim, pi, LaborOption::Pickup { pile_id })
}

pub fn start_wander(sim: &mut Settlement, pi: usize) {
    let c = clampi(sim.people[pi].cell_col() + sim.rng.int(-4, 4), 0, sim.world().cols - 1);
    let r = clampi(sim.people[pi].cell_row() + sim.rng.int(-4, 4), 0, sim.world().rows - 1);
    let timer = sim.rng.range(1.5, 5.0);
    sim.people[pi].task = Some(Task::Idle { timer });
    if sim.walkable(c, r) {
        path_to(sim, pi, c, r);
    }
}

/// Picks the plant with the best mass for the walk, so camps eat their way
/// outward instead of everyone crossing the map for one tree.
fn start_harvest(
    sim: &mut Settlement,
    state: &State,
    pi: usize,
    work: Option<usize>,
    job: HarvestJob,
) -> bool {
    let origin = match work {
        Some(bi) => (sim.buildings[bi].col, sim.buildings[bi].row),
        None => (sim.people[pi].cell_col(), sim.people[pi].cell_row()),
    };
    // Two passes: the camp's own range first, then a long walk when the ground
    // around it has been stripped bare.
    let best = pick_plant(sim, state, pi, &job, origin, job.radius)
        .or_else(|| pick_plant(sim, state, pi, &job, origin, job.radius * 3.0));
    let plant_id = match best {
        Some(id) => id,
        None => return false,
    };
    let index = match sim.plant_sim.plant_index(plant_id) {
        Some(i) => i,
        None => return false,
    };
    let (pcol, prow) = {
        let plant = &sim.plant_sim.plants[index];
        (plant.col, plant.row)
    };
    let spot = match sim.free_cell_near(pcol, prow) {
        Some(s) => s,
        None => return false,
    };
    if !path_to(sim, pi, spot.0, spot.1) {
        return false;
    }
    let person_id = sim.people[pi].id;
    sim.plant_sim.plants[index].claimed_by = person_id;
    sim.claim_plant(plant_id, person_id);
    sim.people[pi].task = Some(Task::Harvest {
        plant_id,
        hint: index,
        yields: job.yields,
        regrow: job.regrow,
        phase: Phase::Approach,
        timer: 0.0,
    });
    true
}

/// Best mass for the walk, read out of the coarse plant index so a camp on a
/// map with fifty thousand plants only looks at the ones near it.
fn pick_plant(
    sim: &Settlement,
    state: &State,
    pi: usize,
    job: &HarvestJob,
    origin: (i32, i32),
    radius: f64,
) -> Option<i32> {
    let person_id = sim.people[pi].id;
    let min = state.civ.work.min_harvest_mass;
    let mut best = None;
    let mut best_score = 0.0;
    sim.plant_index.near(origin.0, origin.1, radius, |mark| {
        if !job.classes.contains(&mark.class) {
            return;
        }
        // A species somebody has been cutting by hand is one worth going out
        // of the way for, and worth taking smaller than the camp would have
        // bothered with.
        let interest = mark.lore as f64;
        if (mark.mass as f64) < min * lore_patience(interest) {
            return;
        }
        if mark.claimed_by != 0 && mark.claimed_by != person_id {
            return;
        }
        let d = ((mark.col - origin.0) as f64).hypot((mark.row - origin.1) as f64);
        if d > radius {
            return;
        }
        let score = mark.mass as f64 * lore_weight(interest) / (2.0 + d);
        if score > best_score {
            best_score = score;
            best = Some(mark.id);
        }
    });
    best
}

pub fn start_mine(sim: &mut Settlement, pi: usize, bi: usize) -> bool {
    let (deposit, yields) = match sim.buildings[bi].def.job {
        Some(Job::Mine { deposit, yields }) => (deposit, yields),
        _ => return false,
    };
    let radius = if sim.buildings[bi].def.radius > 0.0 {
        sim.buildings[bi].def.radius
    } else {
        12.0
    };
    let (col, row) = (sim.buildings[bi].col, sim.buildings[bi].row);
    let di = match sim.terrain.find_deposit(deposit, col, row, radius) {
        Some(d) => d,
        None => return false,
    };
    let (dcol, drow, deposit_id) = {
        let dep = &sim.terrain.deposits[di];
        (dep.col, dep.row, dep.id)
    };
    let spot = match sim.free_cell_near(dcol, drow) {
        Some(s) => s,
        None => return false,
    };
    if !path_to(sim, pi, spot.0, spot.1) {
        return false;
    }
    sim.people[pi].task = Some(Task::Mine { deposit_id, yields, phase: Phase::Approach, timer: 0.0 });
    true
}

pub fn start_station(sim: &mut Settlement, pi: usize, bi: usize) -> bool {
    let person_id = sim.people[pi].id;
    let at = sim.work_spot(bi, person_id);
    let id = sim.buildings[bi].id;
    sim.people[pi].task = Some(Task::Station { building_id: id, phase: Phase::Approach, timer: 0.0 });
    if path_to(sim, pi, at.0, at.1) {
        return true;
    }
    if path_to_building(sim, pi, bi) {
        return true;
    }
    sim.people[pi].clear_task();
    false
}

/// Sends somebody to the nearest bank with a bucket, on the way back to a
/// farm. False when there is no water anywhere near, which is when a farm has
/// to make do with the rain it is not getting.
pub fn start_water(sim: &mut Settlement, state: &State, pi: usize, bi: usize) -> bool {
    let id = sim.buildings[bi].id;
    // One at a time. A farm with three hands on it would otherwise send all
    // three to the river and work nobody's field.
    let already = sim.people.iter().any(|p| {
        matches!(p.task, Some(Task::Water { building_id, .. }) if building_id == id)
    });
    if already {
        return false;
    }
    let (cc, cr) = (sim.buildings[bi].col, sim.buildings[bi].row);
    let reach = (state.civ.work.farm_soak_reach.max(1)) * 12;
    let bank = match sim.nearest_water(cc, cr, reach) {
        Some(b) => b,
        None => return false,
    };
    sim.people[pi].task =
        Some(Task::Water { building_id: id, full: false, phase: Phase::Approach });
    if path_to(sim, pi, bank.0, bank.1) {
        return true;
    }
    sim.people[pi].clear_task();
    false
}

pub fn start_deliver(sim: &mut Settlement, state: &State, pi: usize) {
    let ci = sim.colony_of(pi);
    let colony = sim.colonies[ci].id;
    let (cc, cr) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    let store = match sim.nearest_store(colony, cc, cr) {
        Some(bi) => bi,
        None => {
            sim.people[pi].task = Some(Task::Idle { timer: 2.0 });
            return;
        }
    };
    let id = sim.buildings[store].id;
    sim.people[pi].task = Some(Task::Deliver { to_id: id });
    if !path_to_building(sim, pi, store) {
        let (res, n) = sim.people[pi].drop_load();
        if let Some(res) = res {
            sim.deposit(state, ci, res, n, None);
        }
        sim.people[pi].clear_task();
    }
}

#[derive(Clone, Copy)]
enum LaborOption {
    Pickup { pile_id: i32 },
    Build { building_id: i32 },
    HaulOut { res: Res, amount: f64, from_id: i32 },
    Haul { res: Res, amount: f64, to_id: i32, target: HaulTarget },
}

/// Laborers keep their own colony's sites and workshops fed. The scan is cheap
/// enough to run per idle person, and it always picks the nearest useful thing
/// to do.
pub fn start_labor(sim: &mut Settlement, state: &State, pi: usize) -> bool {
    let ci = sim.colony_of(pi);
    let colony = sim.colonies[ci].id;
    let cap = carry_limit(&state.civ.people, &sim.mods_of(ci));
    let person_id = sim.people[pi].id;
    let (px, py) = (sim.people[pi].x, sim.people[pi].y);
    // Every option is collected and tried in order rather than only the best
    // one: a single unreachable load used to block hauling and building
    // entirely, because the person kept picking it and kept failing to path.
    let mut options: Vec<(f64, LaborOption)> = Vec::new();

    // Nobody walks across the map for a pile another town will get to. The
    // nearest one that is too far is kept aside rather than dropped: out of
    // reach is not the same as not worth having, and a town with nothing
    // nearer to be doing would rather fetch it than stand about.
    let mut anything_near = false;
    let mut far: Option<(f64, i32)> = None;
    for pile in &sim.piles {
        if pile.claimed_by != 0 && pile.claimed_by != person_id {
            continue;
        }
        if !sim.wanted(state, ci, pile.res) {
            continue;
        }
        let d = (pile.col as f64 - px).hypot(pile.row as f64 - py);
        if d > fetch_reach(state, pile.by_hand) {
            match far {
                Some((near, _)) if near <= d => {}
                _ => far = Some((d, pile.id)),
            }
            continue;
        }
        anything_near = true;
        let asked = if pile.by_hand { 12.0 } else { 0.0 };
        options.push((19.0 + asked - d * 0.2, LaborOption::Pickup { pile_id: pile.id }));
    }
    if !anything_near {
        if let Some((d, id)) = far {
            options.push((FAR_PILE_SCORE - d * 0.02, LaborOption::Pickup { pile_id: id }));
        }
    }

    for (si, site) in sim.buildings.iter().enumerate() {
        if site.built || site.colony != colony {
            continue;
        }
        let d = (site.col as f64 - px).hypot(site.row as f64 - py);
        for &(res, need) in &site.cost {
            let have = site.delivered[res as usize] + site.incoming[res as usize];
            let short = need - have;
            if short <= 0.0 {
                continue;
            }
            let take = short.min(cap).min(sim.available_stock(ci, res));
            if take <= 0.0 {
                continue;
            }
            options.push((
                20.0 - d * 0.2,
                LaborOption::Haul { res, amount: take, to_id: site.id, target: HaulTarget::Site },
            ));
        }
        if sim.site_ready(si) && site.builders < 3 {
            options.push((24.0 - d * 0.2, LaborOption::Build { building_id: site.id }));
        }
    }

    for b in &sim.buildings {
        if !b.built || b.colony != colony {
            continue;
        }
        let job = match &b.def.job {
            Some(j) => j,
            None => continue,
        };
        let d = (b.col as f64 - px).hypot(b.row as f64 - py);
        match job {
            Job::Craft { input, .. } => {
                if !b.workers.is_empty() {
                    for &(res, n) in input.iter() {
                        let want = (n * 3.0 * state.civ.work.restock_share).ceil();
                        let have = b.inv[res as usize] + b.reserved_in[res as usize];
                        if have >= want {
                            continue;
                        }
                        let take = (want - have).min(cap).min(sim.available_stock(ci, res));
                        if take <= 0.0 {
                            continue;
                        }
                        options.push((
                            14.0 - d * 0.2,
                            LaborOption::Haul { res, amount: take, to_id: b.id, target: HaulTarget::Input },
                        ));
                    }
                }
            }
            // An inn without food in the larder is a room with no supper.
            Job::Innkeep => {
                let want = (b.def.rooms as f64 * state.civ.people.meal_size).ceil();
                let have = b.inv[Res::Food as usize] + b.reserved_in[Res::Food as usize];
                if have < want {
                    let take = (want - have).min(cap).min(sim.available_stock(ci, Res::Food));
                    if take > 0.0 {
                        options.push((
                            13.0 - d * 0.2,
                            LaborOption::Haul {
                                res: Res::Food,
                                amount: take,
                                to_id: b.id,
                                target: HaulTarget::Input,
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
        // One sum before twelve tests: on a map with hundreds of buildings
        // this scan runs per idle settler per decision, and almost every bench
        // it looks at is empty.
        if b.out_load() > 0.0 {
            for res in crate::civ::resources::RES_IDS {
                let n = b.out[res as usize];
                if n <= 0.0 {
                    continue;
                }
                let free = n - b.reserved_out[res as usize];
                if free < 1.0 {
                    continue;
                }
                options.push((
                    16.0 - d * 0.2,
                    LaborOption::HaulOut { res, amount: free.min(cap), from_id: b.id },
                ));
            }
        }
    }

    options.sort_by(|a, z| z.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, option) in options.into_iter().take(8) {
        if take_labor_task(sim, pi, option) {
            return true;
        }
    }
    false
}

fn take_labor_task(sim: &mut Settlement, pi: usize, best: LaborOption) -> bool {
    let ci = sim.colony_of(pi);
    let colony = sim.colonies[ci].id;
    match best {
        LaborOption::Pickup { pile_id } => {
            let index = match sim.pile_index(pile_id) {
                Some(i) => i,
                None => return false,
            };
            let (col, row) = (sim.piles[index].col, sim.piles[index].row);
            let spot = match sim.free_cell_near(col, row) {
                Some(s) => s,
                None => return false,
            };
            if !path_to(sim, pi, spot.0, spot.1) {
                return false;
            }
            sim.piles[index].claimed_by = sim.people[pi].id;
            sim.people[pi].task = Some(Task::Pickup { pile_id });
            true
        }
        LaborOption::Build { building_id } => {
            let bi = match sim.building_index(building_id) {
                Some(bi) => bi,
                None => return false,
            };
            if !path_to_building(sim, pi, bi) {
                return false;
            }
            sim.buildings[bi].builders += 1;
            sim.people[pi].task = Some(Task::Build { building_id, phase: Phase::Approach });
            true
        }
        LaborOption::HaulOut { res, amount, from_id } => {
            let bi = match sim.building_index(from_id) {
                Some(bi) => bi,
                None => return false,
            };
            if !path_to_building(sim, pi, bi) {
                return false;
            }
            sim.buildings[bi].reserved_out[res as usize] += amount;
            sim.people[pi].task = Some(Task::Haul {
                res,
                amount,
                from_id,
                to_id: 0,
                target: HaulTarget::Output,
                phase: Phase::Approach,
            });
            true
        }
        LaborOption::Haul { res, amount, to_id, target } => {
            let dest = sim.building_index(to_id);
            let (cc, cr) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
            let store = sim.nearest_store(colony, cc, cr);
            let (dest, store) = match (dest, store) {
                (Some(d), Some(s)) => (d, s),
                _ => return false,
            };
            if !path_to_building(sim, pi, store) {
                return false;
            }
            sim.reserve_stock(ci, res, amount);
            let i = res as usize;
            if target == HaulTarget::Site {
                sim.buildings[dest].incoming[i] += amount;
            } else {
                sim.buildings[dest].reserved_in[i] += amount;
            }
            let from_id = sim.buildings[store].id;
            sim.people[pi].task = Some(Task::Haul {
                res,
                amount,
                from_id,
                to_id,
                target,
                phase: Phase::Approach,
            });
            true
        }
    }
}

pub fn run_task(sim: &mut Settlement, state: &State, pi: usize, dt: f64) {
    let task = match sim.people[pi].task.clone() {
        Some(t) => t,
        None => return,
    };
    let ci = sim.colony_of(pi);
    match task {
        Task::Idle { .. } => {
            if walk(sim, state, pi, dt, 0.8) {
                if let Some(Task::Idle { timer }) = &mut sim.people[pi].task {
                    *timer -= dt;
                    if *timer <= 0.0 {
                        sim.people[pi].clear_task();
                    }
                }
            }
        }
        Task::Water { building_id, full, phase } => {
            if phase != Phase::Approach || !walk(sim, state, pi, dt, 1.0) {
                return;
            }
            let bi = match sim.building_index(building_id) {
                Some(b) => b,
                None => {
                    sim.people[pi].clear_task();
                    return;
                }
            };
            if !full {
                // At the bank: fill and turn round. The bucket is the walk,
                // not a thing anybody has to be given.
                let at = sim.work_spot(bi, sim.people[pi].id);
                sim.people[pi].task =
                    Some(Task::Water { building_id, full: true, phase: Phase::Approach });
                if !path_to(sim, pi, at.0, at.1) && !path_to_building(sim, pi, bi) {
                    sim.people[pi].clear_task();
                }
                return;
            }
            let bucket = state.civ.work.farm_bucket.max(0.0);
            sim.buildings[bi].water = clamp01(sim.buildings[bi].water + bucket);
            sim.buildings[bi].active = sim.time;
            sim.people[pi].clear_task();
        }
        Task::Sleep { building_id, phase, hired } => {
            if phase != Phase::Approach || !walk(sim, state, pi, dt, 1.0) {
                return;
            }
            sim.people[pi].task =
                Some(Task::Sleep { building_id, phase: Phase::Working, hired });
            sim.people[pi].sleeping = true;
            let bi = match sim.building_index(building_id) {
                Some(bi) if sim.buildings[bi].built => bi,
                _ => return,
            };
            if hired {
                // The room and the supper are paid for on arrival, and the coin
                // goes to the town rather than to the innkeeper's pocket.
                let price = state.civ.people.inn_price;
                let paid = sim.people[pi].spend(price);
                sim.colonies[ci].econ.coin += paid;
                let meal = take_stock(&mut sim.buildings[bi].inv, Res::Food, state.civ.people.meal_size);
                if meal > 0.0 {
                    sim.people[pi].eat(meal);
                    sim.colonies[ci].econ.record_consumed(Res::Food, meal);
                }
                sim.buildings[bi].active = sim.time;
                sim.people[pi].happiness = clamp01(sim.people[pi].happiness + 0.05);
            }
            if sim.buildings[bi].def.indoor {
                sim.enter_building(pi, bi);
            }
        }
        Task::Eat { .. } => {
            if !walk(sim, state, pi, dt, 1.0) {
                return;
            }
            let adult = sim.people[pi].adult();
            let meal = state.civ.people.meal_size * if adult { 1.0 } else { 0.6 };
            let colony = sim.colonies[ci].id;
            let has_market = sim.has_market(colony);
            let got = {
                let Settlement { colonies, people, .. } = sim;
                let c = &mut colonies[ci];
                buy_food(&mut c.econ, &mut people[pi], &mut c.stock, meal, has_market)
            };
            if got > 0.0 {
                sim.people[pi].eat(got);
            } else {
                sim.people[pi].happiness = clamp01(sim.people[pi].happiness - 0.1);
            }
            sim.people[pi].clear_task();
        }
        Task::Harvest { plant_id, hint, yields, regrow, phase, .. } => {
            let index = match sim.plant_sim.plant_at(plant_id, hint) {
                // A plant somebody else has already cut is on its way to the
                // ground; there is nothing left to walk to.
                Some(i) if sim.plant_sim.plants[i].standing() => i,
                _ => {
                    sim.people[pi].clear_task();
                    return;
                }
            };
            if index != hint {
                if let Some(Task::Harvest { hint, .. }) = &mut sim.people[pi].task {
                    *hint = index;
                }
            }
            if phase == Phase::Approach {
                if walk(sim, state, pi, dt, 1.0) {
                    if let Some(Task::Harvest { phase, .. }) = &mut sim.people[pi].task {
                        *phase = Phase::Working;
                    }
                }
                return;
            }
            let mods = sim.mods_of(ci);
            let prof = sim.people[pi].profession;
            let rate = state.civ.work.harvest_rate
                * mods.gather
                * state.civ.people.work_rate
                * sim.people[pi].skill_in(prof)
                * (0.7 + sim.people[pi].traits.diligence * 0.6);
            let timer = match &mut sim.people[pi].task {
                Some(Task::Harvest { timer, .. }) => {
                    *timer += rate * dt;
                    *timer
                }
                _ => return,
            };
            do_work(sim, state, pi, rate * dt);
            sim.people[pi].practice(prof, dt);
            let mass = sim.plant_mass(&sim.plant_sim.plants[index]);
            if timer < mass {
                return;
            }
            let cap = carry_limit(&state.civ.people, &mods);
            // A mat that is cut back only gives up the part that was taken; a
            // felled tree gives up all of it.
            let cut_back = regrow > 0.0 && sim.plant_sim.plants[index].size_class == SizeClass::Ground;
            let gain = mass * if cut_back { 1.0 - regrow } else { 1.0 } * mods.yields;
            let (pcol, prow) = (sim.plant_sim.plants[index].col, sim.plant_sim.plants[index].row);
            // Whatever will not fit on one person stays where it fell and has
            // to be carried in later: felling a tree is not the same as having
            // the timber in the store.
            for &(res, per) in yields.iter() {
                // Nothing is stripped from a plant that the colony has no use
                // for; a byproduct it is drowning in is simply left on the
                // plant.
                if !sim.wanted(state, ci, res) {
                    continue;
                }
                let total = (gain * per).round().max(1.0);
                let carried = sim.people[pi].carry;
                let room = match carried.res {
                    Some(have) if have != res => 0.0,
                    _ => cap - carried.n,
                };
                let take = total.min(room).max(0.0);
                if take > 0.0 {
                    sim.people[pi].pick(res, take);
                }
                if total - take > 0.0 {
                    sim.add_pile(pcol, prow, res, total - take);
                }
                sim.colonies[ci].econ.record_produced(res, total);
            }
            sim.take_plant(index, cut_back, regrow);
            sim.people[pi].clear_task();
            if sim.people[pi].carry.n >= cap * 0.9 {
                start_deliver(sim, state, pi);
            }
        }
        Task::Mine { deposit_id, yields, phase, .. } => {
            let di = match sim.terrain.deposit_by_id(deposit_id) {
                Some(d) if sim.terrain.deposits[d].amount > 0.0 => d,
                _ => {
                    sim.people[pi].clear_task();
                    return;
                }
            };
            if phase == Phase::Approach {
                if walk(sim, state, pi, dt, 1.0) {
                    if let Some(Task::Mine { phase, .. }) = &mut sim.people[pi].task {
                        *phase = Phase::Working;
                    }
                }
                return;
            }
            let mods = sim.mods_of(ci);
            let prof = sim.people[pi].profession;
            let rate = state.civ.work.mine_rate
                * mods.gather
                * state.civ.people.work_rate
                * sim.people[pi].skill_in(prof)
                * (0.7 + sim.people[pi].traits.diligence * 0.6);
            do_work(sim, state, pi, rate * dt);
            sim.people[pi].practice(prof, dt);
            if let Some(Task::Mine { timer, .. }) = &mut sim.people[pi].task {
                *timer += rate * dt;
            }
            let cap = carry_limit(&state.civ.people, &mods);
            while let Some(Task::Mine { timer, .. }) = &sim.people[pi].task {
                if *timer < 1.0
                    || sim.people[pi].carry.n >= cap
                    || sim.terrain.deposits[di].amount <= 0.0
                {
                    break;
                }
                if let Some(Task::Mine { timer, .. }) = &mut sim.people[pi].task {
                    *timer -= 1.0;
                }
                if sim.terrain.take(di, 1.0) > 0.0 {
                    for &(res, per) in yields.iter() {
                        let n = (per * mods.yields).round().max(1.0);
                        sim.people[pi].pick(res, n);
                        sim.colonies[ci].econ.record_produced(res, n);
                    }
                }
            }
            if sim.people[pi].carry.n >= cap || sim.terrain.deposits[di].amount <= 0.0 {
                sim.people[pi].clear_task();
                if sim.people[pi].carrying() {
                    start_deliver(sim, state, pi);
                }
            }
        }
        Task::Pickup { pile_id } => {
            let index = match sim.pile_index(pile_id) {
                Some(i) => i,
                None => {
                    sim.people[pi].clear_task();
                    return;
                }
            };
            if !walk(sim, state, pi, dt, 1.0) {
                return;
            }
            let cap = carry_limit(&state.civ.people, &sim.mods_of(ci));
            let res = sim.piles[index].res;
            let carried = sim.people[pi].carry;
            let room = match carried.res {
                Some(have) if have != res => 0.0,
                _ => cap - carried.n,
            };
            let got = sim.take_pile(index, room);
            if got > 0.0 {
                sim.people[pi].pick(res, got);
            }
            if let Some(index) = sim.pile_index(pile_id) {
                sim.piles[index].claimed_by = 0;
            }
            sim.people[pi].clear_task();
            if sim.people[pi].carrying() {
                start_deliver(sim, state, pi);
            }
        }
        Task::Deliver { .. } => {
            if !walk(sim, state, pi, dt, 1.0) {
                return;
            }
            let (res, n) = sim.people[pi].drop_load();
            if let Some(res) = res {
                let put = sim.deposit(state, ci, res, n, None);
                if put < n {
                    sim.people[pi].pick(res, n - put);
                }
            }
            sim.people[pi].clear_task();
        }
        Task::Haul { res, amount, from_id, to_id, target, phase } => {
            if phase == Phase::Approach {
                if !walk(sim, state, pi, dt, 1.0) {
                    return;
                }
                if target == HaulTarget::Output {
                    let bi = match sim.building_index(from_id) {
                        Some(bi) => bi,
                        None => {
                            abandon_task(sim, pi);
                            return;
                        }
                    };
                    let i = res as usize;
                    let got = amount.min(sim.buildings[bi].out[i]);
                    sim.buildings[bi].out[i] -= got;
                    sim.buildings[bi].reserved_out[i] =
                        (sim.buildings[bi].reserved_out[i] - amount).max(0.0);
                    if got <= 0.0 {
                        sim.people[pi].clear_task();
                        return;
                    }
                    sim.people[pi].pick(res, got);
                    let colony = sim.colonies[ci].id;
                    let (cc, cr) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
                    let store = match sim.nearest_store(colony, cc, cr) {
                        Some(s) => s,
                        None => {
                            sim.people[pi].clear_task();
                            return;
                        }
                    };
                    let store_id = sim.buildings[store].id;
                    sim.people[pi].task = Some(Task::Haul {
                        res,
                        amount: got,
                        from_id,
                        to_id: store_id,
                        target,
                        phase: Phase::Working,
                    });
                    let at = sim.access_cell(store);
                    if !path_to(sim, pi, at.0, at.1) {
                        start_deliver(sim, state, pi);
                    }
                    return;
                }
                let got = take_stock(&mut sim.colonies[ci].stock, res, amount);
                sim.release_stock(ci, res, amount);
                if got <= 0.0 {
                    abandon_task(sim, pi);
                    return;
                }
                if target == HaulTarget::Stall {
                    // A keeper buys their stock like anybody else: their own
                    // coin, at the town's price, into the town's treasury.
                    let price = sim.colonies[ci].econ.price_of(res) * got;
                    let paid = sim.people[pi].spend(price);
                    sim.colonies[ci].econ.coin += paid;
                }
                sim.people[pi].pick(res, got);
                let dest = match sim.building_index(to_id) {
                    Some(d) => d,
                    None => {
                        start_deliver(sim, state, pi);
                        return;
                    }
                };
                sim.people[pi].task = Some(Task::Haul {
                    res,
                    amount: got,
                    from_id,
                    to_id,
                    target,
                    phase: Phase::Working,
                });
                let at = sim.access_cell(dest);
                if !path_to(sim, pi, at.0, at.1) {
                    start_deliver(sim, state, pi);
                }
                return;
            }
            if !walk(sim, state, pi, dt, 1.0) {
                return;
            }
            let dest = sim.building_index(to_id);
            let (load_res, load_n) = sim.people[pi].drop_load();
            let dest = match dest {
                Some(d) => d,
                None => {
                    if let Some(load_res) = load_res {
                        sim.deposit(state, ci, load_res, load_n, None);
                    }
                    sim.people[pi].clear_task();
                    return;
                }
            };
            let i = res as usize;
            match target {
                HaulTarget::Site => {
                    sim.buildings[dest].delivered[i] += load_n;
                    sim.buildings[dest].incoming[i] = (sim.buildings[dest].incoming[i] - amount).max(0.0);
                }
                HaulTarget::Input | HaulTarget::Stall => {
                    sim.buildings[dest].inv[i] += load_n;
                    sim.buildings[dest].reserved_in[i] =
                        (sim.buildings[dest].reserved_in[i] - amount).max(0.0);
                }
                HaulTarget::Output => {
                    if let Some(load_res) = load_res {
                        sim.deposit(state, ci, load_res, load_n, None);
                    }
                }
            }
            sim.people[pi].clear_task();
        }
        Task::Build { building_id, phase } => {
            let bi = match sim.building_index(building_id) {
                Some(bi) if !sim.buildings[bi].built => bi,
                _ => {
                    abandon_task(sim, pi);
                    return;
                }
            };
            if phase == Phase::Approach {
                if walk(sim, state, pi, dt, 1.0) {
                    sim.people[pi].task = Some(Task::Build { building_id, phase: Phase::Working });
                }
                return;
            }
            if !sim.site_ready(bi) {
                abandon_task(sim, pi);
                return;
            }
            let rate = state.civ.work.build_rate
                * sim.mods_of(ci).build
                * state.civ.people.work_rate
                * sim.people[pi].skill
                * (0.7 + sim.people[pi].traits.diligence * 0.6);
            sim.buildings[bi].work_done += rate * dt;
            sim.buildings[bi].active = sim.time;
            do_work(sim, state, pi, rate * dt);
            sim.buffer_dirty = true;
            if sim.buildings[bi].work_done >= sim.buildings[bi].work {
                sim.buildings[bi].builders = (sim.buildings[bi].builders - 1).max(0);
                let cost = sim.buildings[bi].cost.clone();
                let delivered = sim.buildings[bi].delivered;
                for (res, n) in cost {
                    sim.colonies[ci].econ.record_consumed(res, n.min(delivered[res as usize]));
                }
                sim.finish_building(state, bi);
                sim.people[pi].clear_task();
            }
        }
        Task::Station { building_id, phase, .. } => {
            let bi = match sim.building_index(building_id) {
                Some(bi) if sim.buildings[bi].built => bi,
                _ => {
                    sim.people[pi].clear_task();
                    return;
                }
            };
            if phase == Phase::Approach {
                if walk(sim, state, pi, dt, 1.0) {
                    if let Some(Task::Station { phase, .. }) = &mut sim.people[pi].task {
                        *phase = Phase::Working;
                    }
                    if sim.buildings[bi].def.indoor {
                        sim.enter_building(pi, bi);
                    }
                }
                return;
            }
            // A worker standing next to a full output bench carries the load in
            // themselves rather than waiting for a hauler to notice.
            let mods = sim.mods_of(ci);
            let cap = carry_limit(&state.civ.people, &mods);
            if !sim.people[pi].carrying() && sim.buildings[bi].out_load() >= cap * 0.75 {
                for res in crate::civ::resources::RES_IDS {
                    let n = sim.buildings[bi].out[res as usize];
                    if n < 1.0 {
                        continue;
                    }
                    let take = n.floor().min(cap - sim.people[pi].carry.n);
                    if take <= 0.0 {
                        continue;
                    }
                    sim.buildings[bi].out[res as usize] = n - take;
                    sim.people[pi].pick(res, take);
                    break;
                }
                if sim.people[pi].carrying() {
                    start_deliver(sim, state, pi);
                    return;
                }
            }
            let job = match sim.buildings[bi].def.job {
                Some(j) => j,
                None => {
                    sim.people[pi].clear_task();
                    return;
                }
            };
            let prof = sim.people[pi].profession;
            let diligence = 0.7 + sim.people[pi].traits.diligence * 0.6;
            match job {
                Job::Craft { input, output, time } => {
                    if !sim.craft_ready(bi) && sim.buildings[bi].craft_progress <= 0.0 {
                        sim.people[pi].clear_task();
                        return;
                    }
                    let rate = state.civ.work.craft_rate
                        * mods.craft
                        * state.civ.people.work_rate
                        * sim.people[pi].skill_in(prof)
                        * diligence;
                    if sim.buildings[bi].craft_progress <= 0.0 && sim.craft_ready(bi) {
                        for &(res, n) in input.iter() {
                            sim.buildings[bi].inv[res as usize] -= n;
                            sim.colonies[ci].econ.record_consumed(res, n);
                        }
                        sim.buildings[bi].craft_progress = 0.0001;
                    }
                    if sim.buildings[bi].craft_progress > 0.0 {
                        sim.buildings[bi].craft_progress += (rate * dt) / time.max(0.1);
                        sim.buildings[bi].active = sim.time;
                        do_work(sim, state, pi, rate * dt);
                        if sim.buildings[bi].craft_progress >= 1.0 {
                            sim.buildings[bi].craft_progress = 0.0;
                            for &(res, n) in output.iter() {
                                sim.buildings[bi].out[res as usize] += n;
                                sim.colonies[ci].econ.record_produced(res, n);
                            }
                        }
                    }
                }
                Job::Farm { .. } => {
                    let rate = state.civ.work.farm_rate
                        * mods.farm
                        * state.civ.people.work_rate
                        * sim.people[pi].skill_in(prof)
                        * diligence;
                    let fert = sim.farm_fertility(bi);
                    // Working a field dries it out, and a dry field is a poor
                    // one rather than a barren one.
                    let wet = sim.farm_water_factor(state, bi);
                    let used = state.civ.work.farm_water_use * dt;
                    sim.buildings[bi].water = clamp01(sim.buildings[bi].water - used);
                    sim.buildings[bi].craft_progress += rate * dt * fert * wet;
                    sim.buildings[bi].active = sim.time;
                    do_work(sim, state, pi, rate * dt);
                    while sim.buildings[bi].craft_progress >= 1.0 {
                        sim.buildings[bi].craft_progress -= 1.0;
                        sim.buildings[bi].out[Res::Food as usize] += 1.0;
                        sim.colonies[ci].econ.record_produced(Res::Food, 1.0);
                    }
                }
                Job::Research => {
                    // A curious scholar is worth more at a desk than a dutiful
                    // one, which is the only place the trait pays out.
                    let rate = state.civ.tech.research_per_scholar
                        * mods.research
                        * sim.people[pi].skill_in(prof)
                        * (0.6 + sim.people[pi].traits.curiosity * 0.8);
                    sim.colonies[ci].tech.points += rate * dt;
                    sim.people[pi].literacy = (sim.people[pi].literacy + dt * 0.01).min(1.0);
                    sim.buildings[bi].active = sim.time;
                    do_work(sim, state, pi, rate * dt);
                }
                Job::Trade => {
                    sim.buildings[bi].active = sim.time;
                    do_work(sim, state, pi, 0.5 * dt);
                }
                Job::Innkeep => {
                    // The takings follow the rooms that are occupied, and the
                    // kitchen turns the larder into them.
                    let guests = sim.buildings[bi].guests.len() as f64;
                    if guests > 0.0 {
                        sim.buildings[bi].active = sim.time;
                    }
                    do_work(sim, state, pi, (0.3 + guests * 0.2) * dt);
                }
                Job::Ferry => {
                    sim.buildings[bi].active = sim.time;
                    do_work(sim, state, pi, 0.4 * dt);
                }
                // A keeper draws no wage from the town: what they make is the
                // margin, and it only arrives when somebody walks up and buys.
                // Standing here is what keeps the counter open.
                Job::Sell => {
                    if sim.buildings[bi].inv.iter().sum::<f64>() > 0.0 {
                        sim.buildings[bi].active = sim.time;
                    }
                }
                // Gathering jobs are never worked from a station: the person is
                // out at the tree or the seam.
                Job::Harvest { .. } | Job::Mine { .. } => {
                    sim.people[pi].clear_task();
                    return;
                }
            }
            sim.people[pi].practice(prof, dt);
            // Stations are open ended; step away now and then so the sim can
            // reconsider what this person should be doing.
            let done = match &mut sim.people[pi].task {
                Some(Task::Station { timer, .. }) => {
                    *timer += dt;
                    *timer > 6.0
                }
                _ => false,
            };
            if done {
                sim.leave_building(pi);
                sim.people[pi].clear_task();
            }
        }
        Task::Shop { building_id, want } => {
            let bi = match sim.building_index(building_id) {
                Some(bi) if sim.buildings[bi].built => bi,
                _ => {
                    sim.people[pi].clear_task();
                    return;
                }
            };
            if !walk(sim, state, pi, dt, 1.0) {
                return;
            }
            // The ware they came for, or the dearest thing on the counter they
            // can afford: a browser buys the best of what is out.
            let mut choice: Option<(Res, f64)> = None;
            for &res in sim.buildings[bi].def.sells {
                if want.is_some_and(|w| w != res) || sim.buildings[bi].inv[res as usize] < 1.0 {
                    continue;
                }
                let price = sim.stall_price(state, bi, res);
                if price > sim.people[pi].coin {
                    continue;
                }
                if choice.is_none_or(|(_, best)| price > best) {
                    choice = Some((res, price));
                }
            }
            let (res, unit) = match choice {
                Some(c) => c,
                None => {
                    // A wasted walk. The cooldown is what stops a settler with
                    // no coin pacing back to the same empty counter forever.
                    sim.people[pi].shop_cooldown = state.civ.people.day_length * 0.25;
                    sim.people[pi].clear_task();
                    return;
                }
            };
            let wanted = if res == Res::Food { state.civ.people.meal_size } else { 1.0 };
            let units = wanted
                .min(sim.buildings[bi].inv[res as usize])
                .min(sim.people[pi].coin / unit.max(0.01));
            if units <= 0.0 {
                sim.people[pi].shop_cooldown = state.civ.people.day_length * 0.25;
                sim.people[pi].clear_task();
                return;
            }
            let paid = sim.people[pi].spend(unit * units);
            sim.buildings[bi].inv[res as usize] -= units;
            sim.buildings[bi].active = sim.time;
            let owner = sim.buildings[bi].owner;
            // The takings are the keeper's. Nothing about this passes through
            // the treasury, which is the point of a stall.
            if let Some(oi) = sim.people.index_of(owner) {
                sim.people[oi].earn(paid);
                sim.people[oi].practice(Profession::Shopkeeper, units * 2.0);
            }
            sim.colonies[ci].econ.record_consumed(res, units);
            if res == Res::Food {
                sim.people[pi].eat(units);
            } else {
                // Something bought for its own sake, which is the only use a
                // settler has for coin beyond a roof and a meal.
                sim.people[pi].happiness = clamp01(sim.people[pi].happiness + 0.08);
                let day = sim.day;
                let what = res.label().to_lowercase();
                let from = sim.people.get(owner).map(|q| q.given.clone());
                if let Some(from) = from {
                    sim.people[pi].log(day, format!("bought {what} from {from}"));
                }
            }
            sim.people[pi].shop_cooldown = state.civ.people.day_length * 0.5;
            sim.people[pi].clear_task();
        }
    }
}

/// Wages are a market phenomenon here: before a colony has a market, work is
/// subsistence and no coin moves at all. That is also why a settlement with no
/// market never sees anybody rebuild their hut into a house.
pub fn do_work(sim: &mut Settlement, state: &State, pi: usize, units: f64) {
    let ci = sim.colony_of(pi);
    let colony = sim.colonies[ci].id;
    if !sim.has_market(colony) {
        return;
    }
    let keep = state.civ.people.savings_share;
    let Settlement { colonies, people, .. } = sim;
    pay_wage(&mut colonies[ci].econ, &state.civ.economy, &mut people[pi], units, keep);
}

