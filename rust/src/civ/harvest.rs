//! Cutting by hand.
//!
//! The settlement gathers on its own, and everything it gathers is carried by
//! somebody who decided to. This is the one way in: hold the pointer over
//! something growing and it comes down, and what it was worth is left lying
//! where it stood for the town to fetch.
//!
//! Nothing here moves material into a store. A cut plant becomes piles on the
//! ground, exactly as an overfull load does, so the rest of the settlement
//! needs no special case for it: hands that were already going to pick loads up
//! pick these up too, only sooner.
//!
//! The second half is what the towns make of it. Every cut is remembered
//! against the species it was made on, and a species that has been cut for is
//! one the gatherers start walking to before they would have bothered: what is
//! taught by hand is what gets harvested.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::civ::buildings::{Job, BUILDINGS};
use crate::civ::resources::Res;
use crate::civ::settlement::Settlement;
use crate::species::SizeClass;
use crate::state::State;

/// Real seconds a bar stays up once the pointer has left it, and how fast the
/// progress in it runs back out afterward. A cut given up on is not lost the
/// instant the pointer slips off it - a drag across a thicket crosses plants
/// it is not aiming at - but it does not wait about either.
const LINGER: f64 = 0.5;
const FADE: f64 = 1.5;

/// The shortest a cut can take, and how much longer every cell of plant makes
/// it. A tuft comes up at once; a grown tree is a few seconds of holding still.
const BASE_WORK: f64 = 0.3;
const WORK_PER_MASS: f64 = 0.16;
const MAX_WORK: f64 = 4.0;

/// How much of a plant has to be there before the hand can take it, as a share
/// of what a camp insists on. Lower, because somebody pointing at a plant has
/// said they want that one; a camp is only guessing.
const MIN_MASS_SHARE: f64 = 0.5;

/// Plant masses of one species that have to be cut by hand before the lesson is
/// most of the way learned.
const LESSON: f64 = 10.0;

/// How much a fully learned species moves a gatherer: it is worth this much
/// more than its mass says, and they will take one this much smaller than they
/// would otherwise have walked past.
const LORE_WEIGHT: f64 = 1.0;
const LORE_PATIENCE: f64 = 0.6;

/// What the hand has taught the towns.
///
/// Kept on the settlement rather than on a colony: the pointer is not a member
/// of any town, and what it demonstrates on one side of the map is watched from
/// both. The number stored is raw mass cut, so the curve below can be retuned
/// without invalidating a save.
#[derive(Default, Serialize, Deserialize)]
pub struct Lore {
    cut: HashMap<String, f64>,
}

impl Lore {
    pub fn teach(&mut self, species: &str, mass: f64) {
        if mass <= 0.0 {
            return;
        }
        *self.cut.entry(species.to_string()).or_insert(0.0) += mass;
    }

    /// How much the gatherers have taken to a species, from nothing at 0 to
    /// nearly all of it once a few plants of it have come down. Saturating, so
    /// a hundred cuts is not a hundred times the pull of one.
    pub fn interest(&self, species: &str) -> f64 {
        match self.cut.get(species) {
            Some(&mass) => mass / (mass + LESSON),
            None => 0.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cut.is_empty()
    }

    /// Every species with anything learned about it, strongest first. Reading
    /// order is fixed so a panel listing it does not shuffle between frames.
    pub fn known(&self) -> Vec<(&str, f64)> {
        let mut list: Vec<(&str, f64)> = self
            .cut
            .iter()
            .map(|(id, &mass)| (id.as_str(), mass / (mass + LESSON)))
            .collect();
        list.sort_by(|a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(b.0))
        });
        list
    }

    pub fn clear(&mut self) {
        self.cut.clear();
    }
}

/// A plant the pointer is working on, and how far through it is.
///
/// One per plant rather than one in all: a drag across a patch leaves a row of
/// part cut plants behind it, and each of them has to decide on its own whether
/// enough was spent on it.
pub struct HandCut {
    pub plant_id: i32,
    pub col: i32,
    pub row: i32,
    /// The picture, in world pixels, so the bar can be put over the plant
    /// rather than over the cell its stem is in.
    pub height_px: f64,
    pub half_w_px: f64,
    pub done: f64,
    pub work: f64,
    /// Real seconds since the pointer was last on this plant.
    pub idle: f64,
}

impl HandCut {
    pub fn fraction(&self) -> f64 {
        (self.done / self.work.max(0.001)).clamp(0.0, 1.0)
    }

    /// How solid the bar reads: full while it is being worked, thinning out
    /// once the pointer has gone and the progress with it.
    pub fn alpha(&self) -> f64 {
        if self.idle <= LINGER {
            1.0
        } else {
            (1.0 - (self.idle - LINGER) / LINGER).clamp(0.0, 1.0)
        }
    }
}

/// What one finished cut yielded, for the line of text that reports it.
pub struct Cut {
    pub species: String,
    pub gains: Vec<(Res, f64)>,
    /// Whether the plant was cut back and left growing rather than taken away.
    pub cut_back: bool,
}

/// What a plant of this size class gives up, taken from whichever camp fells
/// that kind of thing. Read off the building table rather than written out
/// again, so a hand and a woodcutter never disagree about what a tree is worth.
fn hand_job(class: SizeClass) -> Option<(&'static [(Res, f64)], f64)> {
    BUILDINGS.iter().find_map(|def| match def.job {
        Some(Job::Harvest { classes, yields, regrow }) if classes.contains(&class) => {
            Some((yields, regrow))
        }
        _ => None,
    })
}

/// The least a plant can be and still be worth pointing at.
pub fn min_mass(state: &State) -> f64 {
    (state.civ.work.min_harvest_mass * MIN_MASS_SHARE).max(0.4)
}

/// How long this plant takes to come down.
fn work_for(mass: f64) -> f64 {
    (BASE_WORK + mass * WORK_PER_MASS).clamp(BASE_WORK, MAX_WORK)
}

/// The box a plant fills on the map, in world pixels: how far it stands up out
/// of the ground and how far it spreads either side. Never smaller than a
/// target somebody could hit, so a seedling is still something to point at.
pub fn mark_box(sim: &Settlement, height_px: f64, radius_px: f64) -> (f64, f64) {
    let cell = sim.world().cell_px as f64;
    (height_px.max(cell * 0.5), radius_px.max(cell * 0.4))
}

impl Settlement {
    /// The plant under a point on the ground plane, or nothing there. A tree is
    /// aimed at by its crown as often as by its foot, so the test is against
    /// the box the picture fills rather than against the cell it grows in.
    pub fn harvestable_at(&self, state: &State, gx: f64, gy: f64) -> Option<i32> {
        let world = self.world();
        let (cell, depth) = (world.cell_px as f64, world.depth_px.max(1) as f64);
        let min = min_mass(state);
        // Generous, because a tall plant's crown is many rows above the cell it
        // is rooted in and the buckets are indexed by the root.
        let sweep = 24.0;
        let mut best: Option<(f64, i32)> = None;
        self.plant_index.near(gx as i32, gy as i32, sweep, |mark| {
            let mass = mark.mass as f64;
            if mass < min {
                return;
            }
            let (height_px, half_w_px) =
                mark_box(self, mark.height_px as f64, mark.radius_px as f64);
            let base_x = mark.col as f64 + 0.5;
            let base_y = mark.row as f64 + 0.5;
            // Half a cell of slack all round, so a small plant is not a pixel
            // to hit at any zoom.
            let half_w = half_w_px / cell + 0.5;
            let up = height_px / depth + 0.5;
            let dx = (gx - base_x) / half_w;
            let dy = if gy < base_y { (base_y - gy) / up } else { (gy - base_y) / 0.6 };
            let d = dx * dx + dy * dy;
            if d > 1.0 {
                return;
            }
            match best {
                Some((near, _)) if near <= d => {}
                _ => best = Some((d, mark.id)),
            }
        });
        best.map(|(_, id)| id)
    }

    /// One frame of the pointer being held down. `at` is where it is on the
    /// ground plane, or nothing when it is not pressed; `dt` is real seconds,
    /// because a hand works at the same rate whatever speed the world is being
    /// watched at, and works with the world paused.
    ///
    /// Returns whatever finished this frame, for the line of text that says so.
    pub fn hand_harvest(&mut self, state: &State, at: Option<(f64, f64)>, dt: f64) -> Option<Cut> {
        let touched = at.and_then(|(gx, gy)| self.harvestable_at(state, gx, gy));
        for cut in &mut self.hand {
            cut.idle += dt;
        }
        if let Some(id) = touched {
            self.touch_cut(id, dt);
        }
        // A cut nobody is spending time on leaks back out and takes its bar
        // with it, so a hold that was not long enough leaves no mark.
        for i in (0..self.hand.len()).rev() {
            if self.hand[i].idle <= LINGER {
                continue;
            }
            self.hand[i].done -= dt * FADE * self.hand[i].work;
            if self.hand[i].done <= 0.0 || self.hand[i].idle > LINGER * 2.0 {
                self.hand.remove(i);
            }
        }
        let ready = self.hand.iter().position(|c| c.done >= c.work)?;
        let plant_id = self.hand.remove(ready).plant_id;
        self.reap(state, plant_id)
    }

    /// Puts this frame's work into the plant under the pointer, starting a cut
    /// on it if there was not one.
    fn touch_cut(&mut self, plant_id: i32, dt: f64) {
        if let Some(cut) = self.hand.iter_mut().find(|c| c.plant_id == plant_id) {
            cut.idle = 0.0;
            cut.done += dt;
            return;
        }
        let index = match self.plant_sim.plant_index(plant_id) {
            Some(i) => i,
            None => return,
        };
        let mass = self.plant_mass(&self.plant_sim.plants[index]);
        let plant = &self.plant_sim.plants[index];
        let (height, radius) = (plant.height_px, plant.radius_px);
        let (col, row) = (plant.col, plant.row);
        let (height_px, half_w_px) = mark_box(self, height, radius);
        self.hand.push(HandCut {
            plant_id,
            col,
            row,
            height_px,
            half_w_px,
            done: dt,
            work: work_for(mass),
            idle: 0.0,
        });
    }

    /// Takes a plant down and leaves what it was worth on the ground. Nothing
    /// is carried: the hand has nowhere to put it, which is the whole point of
    /// the tool.
    fn reap(&mut self, state: &State, plant_id: i32) -> Option<Cut> {
        let index = self.plant_sim.plant_index(plant_id).filter(|&i| self.plant_sim.plants[i].alive)?;
        let class = self.plant_sim.plants[index].size_class;
        let (yields, regrow) = hand_job(class)?;
        let mass = self.plant_mass(&self.plant_sim.plants[index]);
        let species = self.plant_sim.plants[index].species_id.clone();
        let (col, row) = (self.plant_sim.plants[index].col, self.plant_sim.plants[index].row);
        // The same rule the camps work under: ground cover gives up only what
        // was taken off it, everything else gives up all of itself.
        let cut_back = regrow > 0.0 && class == SizeClass::Ground;
        let ci = self.colony_near(col, row);
        let yield_mod = ci.map(|ci| self.colonies[ci].mods.yields).unwrap_or(1.0);
        let gain = mass * if cut_back { 1.0 - regrow } else { 1.0 } * yield_mod;

        let mut gains: Vec<(Res, f64)> = Vec::new();
        for &(res, per) in yields.iter() {
            let total = (gain * per).round().max(1.0);
            self.add_hand_pile(col, row, res, total);
            if let Some(ci) = ci {
                self.colonies[ci].econ.record_produced(res, total);
            }
            gains.push((res, total));
        }
        self.take_plant(index, cut_back, regrow);
        self.lore.teach(&species, mass);
        // The index carries both what can be pointed at and what the gatherers
        // have been taught, and this changed each of them.
        self.rebuild_plant_index();
        let name = state
            .find_species(&species)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| species.clone());
        Some(Cut { species: name, gains, cut_back })
    }

    /// What is left of a plant that has been harvested: ground cover is cut
    /// back and grows again, anything else is taken away. Shared by the settler
    /// who did it as a day's work and by the hand that did it directly.
    pub fn take_plant(&mut self, index: usize, cut_back: bool, regrow: f64) {
        if cut_back {
            let plant = &mut self.plant_sim.plants[index];
            plant.radius_px *= regrow;
            plant.height_px *= regrow;
            plant.confined_side = false;
            plant.dirty = true;
            plant.claimed_by = 0;
            let id = plant.id;
            self.plant_sim.raster_queue.push_back(id);
            self.claim_plant(id, 0);
        } else {
            self.plant_sim.remove_plant_at(index);
        }
    }

    /// A load left by the hand. The same pile everything else drops, marked as
    /// asked for, which is what puts it in front of the work a settler would
    /// otherwise have chosen.
    pub fn add_hand_pile(&mut self, col: i32, row: i32, res: Res, n: f64) {
        if let Some(index) = self.add_pile(col, row, res, n) {
            self.piles[index].by_hand = true;
        }
    }

    /// The colony nearest a point, which is whose books a cut is entered in and
    /// whose technologies say what it was worth. Nothing is entered at all
    /// before the first landing.
    pub fn colony_near(&self, col: i32, row: i32) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for (ci, c) in self.colonies.iter().enumerate() {
            if c.abandoned {
                continue;
            }
            let d = ((c.center.0 - col) as f64).hypot((c.center.1 - row) as f64);
            match best {
                Some((near, _)) if near <= d => {}
                _ => best = Some((d, ci)),
            }
        }
        best.map(|(_, ci)| ci)
    }

    /// Lets go of every part cut plant. What a pointer was in the middle of is
    /// not something to come back to after a mode change or a reload.
    pub fn drop_cuts(&mut self) {
        self.hand.clear();
    }
}

/// How much further a gatherer will go for a species that has been cut for
/// them, and how much smaller a specimen of it they will settle for. Both read
/// out of the mark rather than the map, because they are asked once per plant
/// per decision.
pub fn lore_weight(interest: f64) -> f64 {
    1.0 + interest * LORE_WEIGHT
}

pub fn lore_patience(interest: f64) -> f64 {
    1.0 - interest * LORE_PATIENCE
}
