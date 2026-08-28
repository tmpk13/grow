//! A colony: one town's books.
//!
//! The world is one map with one wilderness growing on it, but a settlement is
//! not one town. Every colony keeps its own store, its own treasury and prices,
//! its own research and its own idea of where its center is. Buildings and
//! people carry the id of the colony they belong to, and everything that used
//! to read "the settlement's stock" now reads "this colony's stock".
//!
//! Splitting the books rather than the world is what makes rivers, boats and
//! trade between towns mean anything: two colonies on the same map can be short
//! of different things at the same time.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::civ::economy::{Economy, EconomyConfig};
use crate::civ::resources::{Res, Stock, RES_COUNT};
use crate::civ::tech::{Mods, TechState};
use crate::util::pack_rgba;

/// Distinct banner colors, cycled by colony index, so a person or a building
/// can be read back to its town at a glance.
const BANNERS: [(i32, i32, i32); 8] = [
    (214, 176, 96),
    (108, 168, 214),
    (198, 108, 100),
    (128, 190, 128),
    (188, 136, 200),
    (216, 148, 92),
    (120, 196, 190),
    (206, 206, 214),
];

#[derive(Serialize, Deserialize)]
pub struct Colony {
    pub id: i32,
    pub name: String,
    /// Where the town reads as being: the planner sprawls out from here.
    pub center: (i32, i32),
    pub stock: Stock,
    pub stock_reserved: Stock,
    pub econ: Economy,
    pub tech: TechState,
    pub mods: Mods,
    /// Worked out from `tech` whenever that changes, so a saved colony does
    /// not carry it and reads it back off its own technologies.
    #[serde(skip)]
    pub unlocked: HashSet<&'static str>,
    pub plan_timer: f64,
    pub births: u32,
    pub deaths: u32,
    pub founded_day: i32,
    /// The colony this one split off from, or 0 for the first landing.
    pub parent: i32,
    pub seed: u32,
    pub banner: u32,
    /// Counts down between attempts to send settlers out to found a new town.
    pub expedition_timer: f64,
    /// Counts down between attempts to send a balloon up, when that experiment
    /// is switched on. A town that has never had one runs it down all the same,
    /// so turning the switch on does not make every town launch at once.
    #[serde(default)]
    pub balloon_timer: f64,
    /// True while nobody lives here. The buildings stay standing; the town
    /// stops being planned for, sailed to and grown into.
    pub abandoned: bool,
    /// The day it emptied, so the fact is reported once rather than daily.
    pub emptied_day: Option<i32>,
    /// Cheap answers to questions asked thousands of times a tick. Every one of
    /// these is a fold over the whole population or the whole building list, so
    /// they are computed once per step in `refresh_colonies` instead.
    pub population: usize,
    pub adults: usize,
    pub roofless: usize,
    pub housing: i32,
    pub storage: f64,
    pub has_market: bool,
    /// Indices into the settlement's building list, so finding the nearest
    /// store does not walk every building in the world.
    pub stores: Vec<usize>,
}

impl Colony {
    pub fn new(id: i32, name: String, center: (i32, i32), cfg: &EconomyConfig, seed: u32) -> Self {
        let tech = TechState::default();
        let (r, g, b) = BANNERS[(id.max(1) as usize - 1) % BANNERS.len()];
        Colony {
            id,
            name,
            center,
            stock: [0.0; RES_COUNT],
            stock_reserved: [0.0; RES_COUNT],
            econ: Economy::new(cfg),
            mods: tech.modifiers(),
            unlocked: tech.unlocked_buildings(),
            tech,
            plan_timer: 0.0,
            births: 0,
            deaths: 0,
            founded_day: 0,
            parent: 0,
            seed,
            banner: pack_rgba(r, g, b, 255),
            expedition_timer: 0.0,
            balloon_timer: 0.0,
            abandoned: false,
            emptied_day: None,
            population: 0,
            adults: 0,
            roofless: 0,
            housing: 0,
            storage: 0.0,
            has_market: false,
            stores: Vec::new(),
        }
    }

    pub fn available(&self, res: Res) -> f64 {
        (self.stock[res as usize] - self.stock_reserved[res as usize]).max(0.0)
    }

    pub fn reserve(&mut self, res: Res, n: f64) {
        self.stock_reserved[res as usize] += n;
    }

    pub fn release(&mut self, res: Res, n: f64) {
        let i = res as usize;
        self.stock_reserved[i] = (self.stock_reserved[i] - n).max(0.0);
    }

    /// Research and unlocks are recomputed together, because a tech is only
    /// ever learned by one colony at a time and both derive from the same list.
    pub fn refresh_tech(&mut self) {
        self.mods = self.tech.modifiers();
        self.unlocked = self.tech.unlocked_buildings();
    }
}
