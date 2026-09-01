//! Taking over a settler.
//!
//! One person at a time can be driven by hand rather than by the planner. They
//! keep everything else about being a settler - they age, they get hungry, the
//! dark still works on them - and give up only the deciding: nothing is chosen
//! for them, and where they go is where they are pointed.
//!
//! Steering is a direction rather than a destination, so it is not a path and
//! not a task. The four actions are: what the planner would have done for them,
//! asked for one press at a time.

use serde::{Deserialize, Serialize};

use crate::civ::resources::Res;
use crate::civ::settlement::Settlement;
use crate::civ::tasks::{abandon_task, cut_within_reach, run_task};
use crate::state::State;
use crate::util::clamp01;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ControlConfig {
    /// Whether a settler can be taken over at all. With this off the switch is
    /// not on the toolbar and nobody is being steered.
    pub on: bool,
    /// Draw a stick on the map to steer with. The keys work either way; the
    /// stick is for a screen with no keyboard behind it.
    pub joystick: bool,
    /// How fast they go when driven, against a settler's own walking pace.
    pub speed: f64,
    /// How far the hand reaches for something to cut, pick up or step into, in
    /// cells.
    pub reach: f64,
}

impl Default for ControlConfig {
    fn default() -> Self {
        ControlConfig { on: false, joystick: true, speed: 1.0, reach: 1.6 }
    }
}

/// What the buttons under the map ask for. Each one is a thing the planner
/// would have chosen for them at some point; this is choosing it by hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// Cut down whatever is growing within reach.
    Cut,
    /// Pick up the nearest load on the ground, or put down what they carry.
    Carry,
    /// Step into the building they are standing at, or back out of it.
    Door,
    /// Eat what they are carrying, or what is in the building they are in.
    Eat,
}

pub const ACTS: [Act; 4] = [Act::Cut, Act::Carry, Act::Door, Act::Eat];

impl Act {
    /// What the button says. Two of them swap, because one press does the
    /// thing and the next undoes it.
    pub fn label(self, carrying: bool, indoors: bool) -> &'static str {
        match self {
            Act::Cut => "Cut",
            Act::Carry if carrying => "Put down",
            Act::Carry => "Pick up",
            Act::Door if indoors => "Step out",
            Act::Door => "Step in",
            Act::Eat => "Eat",
        }
    }

    /// The key that does the same thing, and the name the button goes by.
    pub fn key(self) -> &'static str {
        match self {
            Act::Cut => "c",
            Act::Carry => "x",
            Act::Door => "e",
            Act::Eat => "r",
        }
    }
}

/// Hands a settler over to whoever is at the keyboard. Whatever they were
/// doing is dropped: it was chosen for a settler who is no longer choosing.
pub fn take_over(sim: &mut Settlement, id: u32) -> bool {
    let pi = match sim.people.index_of(id) {
        Some(pi) if sim.people[pi].alive => pi,
        _ => return false,
    };
    if sim.driven == id {
        return true;
    }
    let day = sim.day;
    abandon_task(sim, pi);
    sim.people[pi].sleeping = false;
    sim.people[pi].path.clear();
    sim.driven = id;
    sim.drive = (0.0, 0.0);
    sim.people[pi].log(day, "taken in hand");
    true
}

/// Gives them back to the town. They plan for themselves again from wherever
/// they were left standing.
pub fn let_go(sim: &mut Settlement) {
    let id = std::mem::take(&mut sim.driven);
    sim.drive = (0.0, 0.0);
    if let Some(pi) = sim.people.index_of(id) {
        let day = sim.day;
        sim.people[pi].log(day, "left to themselves again");
    }
}

/// The person being driven, if there is one and they are still alive. Clears
/// the hold on somebody who has died, which is the one way it ends by itself.
pub fn driven_index(sim: &mut Settlement) -> Option<usize> {
    if sim.driven == 0 {
        return None;
    }
    match sim.people.index_of(sim.driven) {
        Some(pi) if sim.people[pi].alive => Some(pi),
        _ => {
            sim.driven = 0;
            sim.drive = (0.0, 0.0);
            None
        }
    }
}

/// One tick of somebody being steered.
///
/// A task they were given by a press runs to its end, because cutting a tree
/// down is work and the walk in the middle of it is theirs; pushing the stick
/// drops it, because the hand asking them to move outranks the last thing the
/// hand asked for.
pub fn drive_tick(sim: &mut Settlement, state: &State, pi: usize, dt: f64) {
    let steering = sim.drive != (0.0, 0.0);
    if sim.people[pi].task.is_some() {
        if !steering {
            run_task(sim, state, pi, dt);
            return;
        }
        abandon_task(sim, pi);
    }
    if !steering {
        sim.people[pi].path.clear();
        return;
    }
    sim.leave_building(pi);
    sim.people[pi].path.clear();
    step(sim, state, pi, dt);
}

/// Moving along the stick. Water is crossed rather than walked round, at the
/// same cost a settler pays for swimming, and a wall stops only the part of the
/// push that is into it: the rest slides along it, or a diagonal into a corner
/// would be a dead stop.
fn step(sim: &mut Settlement, state: &State, pi: usize, dt: f64) {
    let pcfg = &state.civ.people;
    let cfg = state.civ.experiments.control;
    let (cc, cr) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    let swimming = sim.in_water(cc, cr);
    let terrain = if swimming { pcfg.swim_speed.clamp(0.05, 1.0) } else { 1.0 };
    let (px, py, energy, health) = {
        let p = &sim.people[pi];
        (p.x, p.y, p.energy, p.health)
    };
    let speed = pcfg.walk_speed
        * cfg.speed.clamp(0.1, 4.0)
        * terrain
        * (0.7 + energy * 0.3)
        * (0.6 + health * 0.4);
    // A stick pushed half way is half a walk; pushed past its edge it is still
    // one walk, which is what stops a diagonal being faster than a straight
    // line.
    let (dx, dy) = sim.drive;
    let len = dx.hypot(dy);
    let push = clamp01(len);
    let (ux, uy) = if len > 1e-6 { (dx / len, dy / len) } else { (0.0, 0.0) };
    let travel = speed * push * dt;
    let (nx, ny) = (px + ux * travel, py + uy * travel);

    let can = |sim: &Settlement, x: f64, y: f64| {
        let (c, r) = (x.floor() as i32, y.floor() as i32);
        sim.in_bounds(c, r) && (sim.walkable(c, r) || sim.in_water(c, r))
    };
    let mut x = px;
    let mut y = py;
    if can(sim, nx, y) {
        x = nx;
    }
    if can(sim, x, ny) {
        y = ny;
    }
    let moved = (x - px).hypot(y - py);
    let p = &mut sim.people[pi];
    p.x = x;
    p.y = y;
    if ux.abs() > 0.01 {
        p.facing = if ux > 0.0 { 1 } else { -1 };
    }
    // The walk cycle counts six frames to the cell, however the cell was
    // covered.
    p.bob += moved * 6.0;

    // A path wears where feet fall, the same as any other walk, and nothing
    // wears into water.
    if moved > 0.0 {
        let (c, r) = (p.cell_col(), p.cell_row());
        if sim.in_bounds(c, r) && !sim.in_water(c, r) {
            let i = sim.idx(c, r);
            sim.traffic[i] = (sim.traffic[i] + (dt * 2.0) as f32).min(20.0);
        }
    }
}

/// One press of one of the buttons under the map. What comes back is what to
/// say about it, which is the whole of the answer: everything else is in the
/// settlement.
pub fn act(sim: &mut Settlement, state: &State, act: Act) -> String {
    let pi = match driven_index(sim) {
        Some(pi) => pi,
        None => return "nobody is being driven".to_string(),
    };
    match act {
        Act::Cut => cut(sim, state, pi),
        Act::Carry => carry(sim, state, pi),
        Act::Door => door(sim, state, pi),
        Act::Eat => eat(sim, state, pi),
    }
}

fn reach_of(state: &State) -> f64 {
    state.civ.experiments.control.reach.clamp(0.5, 8.0)
}

fn cut(sim: &mut Settlement, state: &State, pi: usize) -> String {
    abandon_task(sim, pi);
    if cut_within_reach(sim, state, pi, reach_of(state)) {
        "cutting what is in front of them".to_string()
    } else {
        "nothing within reach to cut".to_string()
    }
}

fn carry(sim: &mut Settlement, state: &State, pi: usize) -> String {
    if sim.people[pi].carrying() {
        let (res, n) = sim.people[pi].drop_load();
        let (c, r) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
        if let Some(res) = res {
            sim.add_pile(c, r, res, n);
            return format!("put down {:.0} {}", n, res.label());
        }
        return "nothing in hand".to_string();
    }
    let reach = reach_of(state);
    let (px, py) = (sim.people[pi].x, sim.people[pi].y);
    let id = sim.people[pi].id;
    let cap = state.civ.people.carry_capacity.max(1.0);
    let mut best: Option<(f64, i32)> = None;
    for pile in &sim.piles {
        if pile.claimed_by != 0 && pile.claimed_by != id {
            continue;
        }
        let d = (pile.col as f64 + 0.5 - px).hypot(pile.row as f64 + 0.5 - py);
        if d > reach {
            continue;
        }
        match best {
            Some((near, _)) if near <= d => {}
            _ => best = Some((d, pile.id)),
        }
    }
    let pile_id = match best {
        Some((_, id)) => id,
        None => return "nothing within reach to pick up".to_string(),
    };
    let index = match sim.pile_index(pile_id) {
        Some(i) => i,
        None => return "nothing within reach to pick up".to_string(),
    };
    let (res, have) = (sim.piles[index].res, sim.piles[index].n);
    let take = have.min(cap);
    sim.piles[index].n -= take;
    if sim.piles[index].n < 0.05 {
        sim.piles.remove(index);
    }
    sim.people[pi].pick(res, take);
    sim.buffer_dirty = true;
    format!("picked up {:.0} {}", take, res.label())
}

fn door(sim: &mut Settlement, state: &State, pi: usize) -> String {
    if sim.people[pi].indoors() {
        sim.leave_building(pi);
        return "stepped outside".to_string();
    }
    let reach = reach_of(state);
    let (px, py) = (sim.people[pi].x, sim.people[pi].y);
    let mut best: Option<(f64, usize)> = None;
    for bi in 0..sim.buildings.len() {
        if !sim.buildings[bi].built || !sim.buildings[bi].def.indoor {
            continue;
        }
        let b = &sim.buildings[bi];
        // Distance to the footprint rather than to its corner, so a long wall
        // is as near as the piece of it they are standing at.
        let cx = px.clamp(b.col as f64, (b.col + b.w) as f64);
        let cy = py.clamp(b.row as f64, (b.row + b.h) as f64);
        let d = (cx - px).hypot(cy - py);
        if d > reach {
            continue;
        }
        match best {
            Some((near, _)) if near <= d => {}
            _ => best = Some((d, bi)),
        }
    }
    match best {
        Some((_, bi)) => {
            let label = sim.buildings[bi].label();
            sim.enter_building(pi, bi);
            format!("stepped into the {label}")
        }
        None => "nothing within reach to step into".to_string(),
    }
}

/// A meal in hand, or out of the building they are standing in. Nothing is
/// bought and nothing is walked to: this is the settler eating what is already
/// there, which is what makes driving one around survivable.
fn eat(sim: &mut Settlement, state: &State, pi: usize) -> String {
    let meal = state.civ.people.meal_size.max(0.1);
    if sim.people[pi].carry.res == Some(Res::Food) && sim.people[pi].carry.n >= meal {
        sim.people[pi].carry.n -= meal;
        if sim.people[pi].carry.n < 0.05 {
            sim.people[pi].drop_load();
        }
        sim.people[pi].eat(meal);
        return "ate what they were carrying".to_string();
    }
    let inside = sim.people[pi].inside;
    if let Some(bi) = sim.building_index(inside) {
        let food = sim.buildings[bi].inv[Res::Food as usize];
        if food >= meal {
            sim.buildings[bi].inv[Res::Food as usize] = food - meal;
            sim.people[pi].eat(meal);
            let label = sim.buildings[bi].label();
            return format!("ate from the {label}");
        }
    }
    "nothing to eat in hand or under this roof".to_string()
}
