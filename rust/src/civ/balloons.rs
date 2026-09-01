//! Hot air balloons: an experiment, not a trade.
//!
//! A town with a school and cloth to spare sews a canopy, burns charcoal under
//! it and sends it up over itself. What can be seen from up there - the shape
//! of the river, where the rock runs out, how far the wood goes - is worth more
//! to the scholars than another day at the bench, so the town's research runs
//! faster for as long as one is aloft.
//!
//! Nothing else in the settlement depends on this. It is reached only through
//! the experiments switch, which is off by default, and with it off no balloon
//! is ever launched and `research_gain` is never asked for. That is the whole
//! point of an experiment: it has to be possible to leave one half thought out
//! without it changing a town somebody was watching.

use serde::{Deserialize, Serialize};

use crate::civ::buildings::Job;
use crate::civ::resources::{take_stock, Res};
use crate::civ::settlement::Settlement;
use crate::state::State;
use crate::util::clamp;

/// One canopy in the air.
///
/// Position is on the ground plane in cells, the same coordinates a person
/// walks in, plus a height in cells above it. Keeping it in ground coordinates
/// is what lets it be drawn in the right place on a projection where up the
/// screen is both height and distance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Balloon {
    pub id: i32,
    pub colony: i32,
    pub x: f64,
    pub y: f64,
    /// Which way the wind has it, in cells per second. Drawn once at launch:
    /// the wind does not change under one flight.
    pub drift: (f64, f64),
    /// Seconds of the flight that have gone, and how long it was to be.
    pub flown: f64,
    pub flight: f64,
    pub seed: u32,
}

impl Balloon {
    /// How high it is now, in cells. It goes up over the first part of the
    /// flight, holds, and comes down over the last part, so a launch and a
    /// landing are both something to watch rather than a balloon blinking on
    /// at altitude.
    pub fn height(&self, ceiling: f64) -> f64 {
        let t = if self.flight > 0.0 { clamp(self.flown / self.flight, 0.0, 1.0) } else { 1.0 };
        const CLIMB: f64 = 0.18;
        let ramp = if t < CLIMB {
            t / CLIMB
        } else if t > 1.0 - CLIMB {
            (1.0 - t) / CLIMB
        } else {
            1.0
        };
        // Eased at both ends, so it leaves the ground and settles back onto it
        // rather than starting and stopping at full speed.
        ceiling * (ramp * ramp * (3.0 - 2.0 * ramp))
    }

    pub fn done(&self) -> bool {
        self.flown >= self.flight
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BalloonConfig {
    pub on: bool,
    /// Simulated seconds between one town's attempts to send one up.
    pub interval: f64,
    /// How long a flight lasts.
    pub flight: f64,
    /// How high it gets, in cells.
    pub ceiling: f64,
    /// How fast the wind carries it, in cells per second.
    pub drift: f64,
    /// Cloth for the canopy and charcoal for the burner, out of the store.
    pub cloth: f64,
    pub fuel: f64,
    /// What one aloft adds to the town's research, as a fraction.
    pub research_gain: f64,
    /// Canopies one town will have in the air at once.
    pub per_town: i32,
}

impl Default for BalloonConfig {
    fn default() -> Self {
        BalloonConfig {
            on: true,
            interval: 420.0,
            flight: 180.0,
            ceiling: 14.0,
            drift: 0.25,
            cloth: 4.0,
            fuel: 3.0,
            research_gain: 0.6,
            per_town: 1,
        }
    }
}

/// Whether balloons are running at all. Both switches, because the block one
/// is what makes an experiment an experiment.
pub fn enabled(state: &State) -> bool {
    state.civ.experiments.on && state.civ.experiments.balloons.on
}

/// What the canopies over this town are worth to its scholars, as a multiplier
/// on research output. One when there is nothing up.
pub fn research_lift(sim: &Settlement, state: &State, colony: i32) -> f64 {
    if !enabled(state) {
        return 1.0;
    }
    let cfg = &state.civ.experiments.balloons;
    let up = sim.balloons.iter().filter(|b| b.colony == colony).count() as f64;
    1.0 + up * cfg.research_gain.max(0.0)
}

/// One tick of every flight, and of every town's decision to start one.
pub fn balloons_tick(sim: &mut Settlement, state: &State, dt: f64) {
    if !enabled(state) {
        // A switch turned off mid flight brings down what is up rather than
        // freezing it in the sky.
        sim.balloons.clear();
        return;
    }
    let cfg = state.civ.experiments.balloons;
    let (cols, rows) = (sim.world().cols as f64, sim.world().rows as f64);
    for i in (0..sim.balloons.len()).rev() {
        let b = &mut sim.balloons[i];
        b.flown += dt;
        b.x = clamp(b.x + b.drift.0 * dt, 0.5, cols - 0.5);
        b.y = clamp(b.y + b.drift.1 * dt, 0.5, rows - 0.5);
        if b.done() {
            sim.balloons.remove(i);
        }
    }
    sim.buffer_dirty = true;

    for ci in 0..sim.colonies.len() {
        sim.colonies[ci].balloon_timer -= dt;
        if sim.colonies[ci].balloon_timer > 0.0 {
            continue;
        }
        sim.colonies[ci].balloon_timer = cfg.interval.max(1.0);
        launch(sim, state, ci);
    }
}

/// Sends one up, if this town can. Everything that would stop it is a plain
/// question with an obvious answer: no school, nothing to see with; no cloth,
/// nothing to sew; one already up, nothing more to learn this afternoon.
fn launch(sim: &mut Settlement, state: &State, ci: usize) {
    let cfg = state.civ.experiments.balloons;
    let colony = sim.colonies[ci].id;
    if sim.colonies[ci].abandoned {
        return;
    }
    let aloft = sim.balloons.iter().filter(|b| b.colony == colony).count() as i32;
    if aloft >= cfg.per_town.max(0) {
        return;
    }
    let school = sim
        .buildings
        .iter()
        .position(|b| b.built && b.colony == colony && matches!(b.def.job, Some(Job::Research)));
    let bi = match school {
        Some(bi) => bi,
        None => return,
    };
    let (cloth, fuel) = (cfg.cloth.max(0.0), cfg.fuel.max(0.0));
    if sim.colonies[ci].stock[Res::Cloth as usize] < cloth
        || sim.colonies[ci].stock[Res::Charcoal as usize] < fuel
    {
        return;
    }
    take_stock(&mut sim.colonies[ci].stock, Res::Cloth, cloth);
    take_stock(&mut sim.colonies[ci].stock, Res::Charcoal, fuel);
    sim.colonies[ci].econ.record_consumed(Res::Cloth, cloth);
    sim.colonies[ci].econ.record_consumed(Res::Charcoal, fuel);

    let at = sim.access_cell(bi);
    let angle = sim.rng.range(0.0, std::f64::consts::PI * 2.0);
    let speed = cfg.drift.max(0.0);
    let id = sim.next_balloon_id;
    sim.next_balloon_id += 1;
    let seed = sim.rng.seed();
    sim.balloons.push(Balloon {
        id,
        colony,
        x: at.0 as f64 + 0.5,
        y: at.1 as f64 + 0.5,
        drift: (angle.cos() * speed, angle.sin() * speed * 0.5),
        flown: 0.0,
        flight: cfg.flight.max(1.0),
        seed,
    });
    let day = sim.day;
    sim.colonies[ci].econ.log_event("a balloon went up".to_string(), day);
}
