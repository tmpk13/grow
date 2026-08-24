//! Settlers.
//!
//! A person is a record plus two mechanical pieces that do not need the rest of
//! the world: needs that drift over time, and movement along a path of cells.
//! Every decision about what to do next is made by the settlement, which is the
//! only thing that can see jobs, buildings and stock.
//!
//! The record half is deliberately fat. A settler carries their parentage, the
//! colony they were born in, a personality that biases what they are good at
//! and how they spend, a skill per trade, the house they own and the log of
//! what happened to them. None of it is needed to make the sim run; all of it
//! is what makes a roster worth reading.

use serde::{Deserialize, Serialize};

use crate::civ::names::{family_name, person_name};
use crate::civ::resources::Res;
use crate::civ::social::Bond;
use crate::civ::tasks::Task;
use crate::civ::tech::Mods;
use crate::rng::Rng;
use crate::util::{clamp, clamp01};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum Profession {
    #[default]
    Laborer,
    Woodcutter,
    Forager,
    Miner,
    Farmer,
    Crafter,
    Scholar,
    Trader,
    Innkeeper,
    Sailor,
    Shopkeeper,
    Child,
}

pub const PROFESSIONS: [Profession; 12] = [
    Profession::Laborer,
    Profession::Woodcutter,
    Profession::Forager,
    Profession::Miner,
    Profession::Farmer,
    Profession::Crafter,
    Profession::Scholar,
    Profession::Trader,
    Profession::Innkeeper,
    Profession::Sailor,
    Profession::Shopkeeper,
    Profession::Child,
];

pub const PROFESSION_COUNT: usize = PROFESSIONS.len();

impl Profession {
    pub fn label(self) -> &'static str {
        match self {
            Profession::Laborer => "Laborer",
            Profession::Woodcutter => "Woodcutter",
            Profession::Forager => "Forager",
            Profession::Miner => "Miner",
            Profession::Farmer => "Farmer",
            Profession::Crafter => "Crafter",
            Profession::Scholar => "Scholar",
            Profession::Trader => "Trader",
            Profession::Innkeeper => "Innkeeper",
            Profession::Sailor => "Sailor",
            Profession::Shopkeeper => "Shopkeeper",
            Profession::Child => "Child",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Personality, drawn once at birth and fixed for life. Every value is in
/// [0,1] and is read by exactly one part of the sim, which keeps them from
/// turning into a single hidden quality score.
#[derive(Clone, Copy, Debug, Default)]
pub struct Traits {
    /// Work rate and how quickly a skill is picked up.
    pub diligence: f64,
    /// How much of a wage is kept rather than spent, and how soon a house is
    /// upgraded.
    pub thrift: f64,
    /// Weighs marrying, moving to a new colony and drinking at an inn.
    pub sociability: f64,
    /// Research output and the chance of being schooled.
    pub curiosity: f64,
    /// Resistance to sickness and to going hungry.
    pub hardiness: f64,
    /// Willingness to leave for a colony expedition or a boat crew.
    pub wanderlust: f64,
}

impl Traits {
    pub fn roll(rng: &mut Rng) -> Traits {
        // Triangular rather than flat: most people are unremarkable, and the
        // tails are what the settlement notices.
        let draw = |rng: &mut Rng| clamp01((rng.next() + rng.next()) * 0.5);
        Traits {
            diligence: draw(rng),
            thrift: draw(rng),
            sociability: draw(rng),
            curiosity: draw(rng),
            hardiness: draw(rng),
            wanderlust: draw(rng),
        }
    }

    /// Children resemble their parents, with room to differ.
    pub fn inherit(a: &Traits, b: &Traits, rng: &mut Rng) -> Traits {
        let blend = |x: f64, y: f64, rng: &mut Rng| {
            clamp01((x + y) * 0.5 + rng.range(-0.22, 0.22))
        };
        Traits {
            diligence: blend(a.diligence, b.diligence, rng),
            thrift: blend(a.thrift, b.thrift, rng),
            sociability: blend(a.sociability, b.sociability, rng),
            curiosity: blend(a.curiosity, b.curiosity, rng),
            hardiness: blend(a.hardiness, b.hardiness, rng),
            wanderlust: blend(a.wanderlust, b.wanderlust, rng),
        }
    }

    pub fn rows(&self) -> [(&'static str, f64); 6] {
        [
            ("diligence", self.diligence),
            ("thrift", self.thrift),
            ("sociability", self.sociability),
            ("curiosity", self.curiosity),
            ("hardiness", self.hardiness),
            ("wanderlust", self.wanderlust),
        ]
    }
}

/// One line of a person's history. Kept short and capped, because every
/// settler who ever lived keeps theirs.
#[derive(Clone, Debug)]
pub struct LifeEvent {
    pub day: i32,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PeopleConfig {
    pub start_population: i32,
    pub walk_speed: f64,
    pub carry_capacity: f64,
    pub work_rate: f64,
    /// Needs are expressed per simulated second; a day is `day_length` seconds,
    /// so a hunger rate of 0.008 means half a day from a full meal to hungry.
    pub day_length: f64,
    pub work_start: f64,
    pub work_end: f64,
    pub hunger_rate: f64,
    pub eat_at: f64,
    pub meal_size: f64,
    pub tire_rate: f64,
    pub sleep_rate: f64,
    pub starve_damage: f64,
    pub heal_rate: f64,
    pub birth_rate: f64,
    pub adult_age: f64,
    pub years_per_day: f64,
    pub lifespan_min: i32,
    pub lifespan_max: i32,
    pub fertile_until: f64,
    pub sickness_rate: f64,
    /// Fraction of adults kept free of a workplace to haul and build.
    pub laborer_share: f64,
    pub road_speed_bonus: f64,
    /// Share of a paid wage a settler keeps rather than handing back to the
    /// colony. Personal coin is what buys a house upgrade.
    pub savings_share: f64,
    /// Coin a settler will part with for a night at an inn.
    pub inn_price: f64,
    /// Age at which a settler starts looking for a spouse.
    pub marry_age: f64,
}

impl Default for PeopleConfig {
    fn default() -> Self {
        PeopleConfig {
            start_population: 5,
            walk_speed: 2.4,
            carry_capacity: 12.0,
            work_rate: 1.0,
            day_length: 120.0,
            work_start: 0.2,
            work_end: 0.8,
            hunger_rate: 0.008,
            eat_at: 0.55,
            meal_size: 2.0,
            tire_rate: 0.006,
            sleep_rate: 0.2,
            starve_damage: 0.006,
            heal_rate: 0.01,
            birth_rate: 0.12,
            adult_age: 12.0,
            years_per_day: 0.45,
            lifespan_min: 62,
            lifespan_max: 92,
            fertile_until: 46.0,
            sickness_rate: 0.008,
            laborer_share: 0.3,
            road_speed_bonus: 0.35,
            savings_share: 0.65,
            inn_price: 2.0,
            marry_age: 17.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Carry {
    pub res: Option<Res>,
    pub n: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Person {
    pub id: u32,
    pub seed: u32,
    pub name: String,
    pub given: String,
    pub family: String,
    /// Which colony this settler belongs to right now. Changes when they join
    /// an expedition or move for work.
    pub colony: i32,
    pub born_in: i32,
    /// Fractional position on the ground plane, in cells.
    pub x: f64,
    pub y: f64,
    pub age: f64,
    pub adult_age: f64,
    pub lifespan: f64,
    pub alive: bool,
    pub cause: Option<&'static str>,
    pub hunger: f64,
    pub energy: f64,
    pub health: f64,
    pub happiness: f64,
    pub coin: f64,
    /// The most coin this person has ever held, which is what "rich" is judged
    /// against once it has been spent on a house.
    pub peak_coin: f64,
    pub home: i32,
    /// The building this settler holds the deed to, which is not always the
    /// one they sleep in: a household shares the owner's roof.
    pub owns: i32,
    pub work: i32,
    /// Building the person is currently standing inside, or 0 for outdoors.
    /// Somebody inside is not drawn, and the building lights up instead.
    pub inside: i32,
    /// Boat the person is aboard, or 0.
    pub aboard: i32,
    pub profession: Profession,
    pub carry: Carry,
    pub task: Option<Task>,
    pub path: Vec<(i32, i32)>,
    pub path_at: usize,
    pub sleeping: bool,
    pub facing: i32,
    pub bob: f64,
    pub skill: f64,
    pub skills: [f32; PROFESSION_COUNT],
    pub wage: f64,
    pub born: i32,
    pub died: i32,
    pub eat_cooldown: f64,
    pub traits: Traits,
    pub mother: u32,
    pub father: u32,
    pub spouse: u32,
    pub children: Vec<u32>,
    pub literacy: f64,
    pub title: Option<&'static str>,
    pub events: Vec<LifeEvent>,
    /// The stall this settler keeps, or 0. A stall is bought and worked by one
    /// person for their own account, which is why it is not the same thing as
    /// the workplace the colony assigns them.
    pub stall: i32,
    /// Stops a settler browsing the stalls on every single decision.
    pub shop_cooldown: f64,
    /// Everyone this settler has met, and what they have come to think of
    /// them. Capped: what matters about a long lived settler is the two dozen
    /// people they actually know.
    pub bonds: Vec<Bond>,
    pub friends: u32,
    pub rivals: u32,
    /// The whole social ledger folded into one number, so the needs tick can
    /// read it without walking the bonds every frame.
    pub regard: f64,
}

impl Person {
    pub fn new(id: u32, col: i32, row: i32, age: f64, rng: &mut Rng) -> Self {
        let seed = rng.seed();
        let given = person_name(rng);
        let family = family_name(rng);
        let name = format!("{given} {family}");
        let hunger = rng.range(0.0, 0.3);
        let energy = rng.range(0.7, 1.0);
        let bob = rng.range(0.0, std::f64::consts::PI * 2.0);
        let traits = Traits::roll(rng);
        Person {
            id,
            seed,
            name,
            given,
            family,
            colony: 0,
            born_in: 0,
            x: col as f64 + 0.5,
            y: row as f64 + 0.5,
            age,
            adult_age: 12.0,
            lifespan: 0.0,
            alive: true,
            cause: None,
            hunger,
            energy,
            health: 1.0,
            happiness: 0.6,
            coin: 0.0,
            peak_coin: 0.0,
            home: 0,
            owns: 0,
            work: 0,
            inside: 0,
            aboard: 0,
            profession: Profession::Laborer,
            carry: Carry::default(),
            task: None,
            path: Vec::new(),
            path_at: 0,
            sleeping: false,
            facing: 1,
            bob,
            skill: 1.0,
            skills: [1.0; PROFESSION_COUNT],
            wage: 0.0,
            born: 0,
            died: -1,
            eat_cooldown: 0.0,
            traits,
            mother: 0,
            father: 0,
            spouse: 0,
            children: Vec::new(),
            literacy: 0.0,
            title: None,
            events: Vec::new(),
            stall: 0,
            shop_cooldown: 0.0,
            bonds: Vec::new(),
            friends: 0,
            rivals: 0,
            regard: 0.0,
        }
    }

    pub fn adult(&self) -> bool {
        self.age >= self.adult_age
    }

    pub fn carrying(&self) -> bool {
        self.carry.n > 0.0
    }

    pub fn indoors(&self) -> bool {
        self.inside != 0
    }

    pub fn cell_col(&self) -> i32 {
        self.x.floor() as i32
    }

    pub fn cell_row(&self) -> i32 {
        self.y.floor() as i32
    }

    pub fn set_path(&mut self, path: Vec<(i32, i32)>) {
        self.path = path;
        self.path_at = 0;
    }

    pub fn clear_task(&mut self) {
        self.task = None;
        self.path.clear();
        self.path_at = 0;
    }

    /// A settler steps out before doing anything that happens outdoors, so the
    /// indoor flag never survives a change of plan.
    pub fn step_outside(&mut self) {
        self.inside = 0;
    }

    /// One line in this person's history. The log is capped: what matters for a
    /// long lived settler is the last dozen things that happened to them.
    pub fn log(&mut self, day: i32, text: impl Into<String>) {
        self.events.push(LifeEvent { day, text: text.into() });
        if self.events.len() > 12 {
            self.events.remove(0);
        }
    }

    pub fn bond_with(&self, who: u32) -> Option<&Bond> {
        self.bonds.iter().find(|b| b.who == who)
    }

    pub fn affinity_for(&self, who: u32) -> f64 {
        self.bond_with(who).map(|b| b.affinity as f64).unwrap_or(0.0)
    }

    /// Files a bond, or finds the one already there, and returns its slot.
    ///
    /// A full memory gives up its faintest bond that is not family, which is
    /// the whole rule: kin and the people somebody has strong feelings about
    /// stay, and the faces stop being remembered. A memory that is nothing but
    /// family has no room for a stranger at all, and grows only for more
    /// family, because who a person is related to is not something they can
    /// forget to make space.
    pub fn remember(&mut self, who: u32, kin: bool, day: i32, cap: usize) -> Option<usize> {
        if who == 0 || who == self.id {
            return None;
        }
        if let Some(slot) = self.bonds.iter().position(|b| b.who == who) {
            return Some(slot);
        }
        let cap = cap.max(4);
        if self.bonds.len() >= cap {
            let faintest = self
                .bonds
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.kin)
                .min_by(|(_, a), (_, b)| {
                    a.affinity
                        .abs()
                        .partial_cmp(&b.affinity.abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i);
            match faintest {
                Some(i) => {
                    self.bonds.remove(i);
                }
                None if kin => {}
                None => return None,
            }
        }
        self.bonds.push(Bond::new(who, kin, day));
        Some(self.bonds.len() - 1)
    }

    /// Marks a bond as family, filing one if there is not one already. Used
    /// where a relationship is a fact of birth or of a wedding rather than
    /// something that grew out of standing next to somebody.
    pub fn bind_kin(&mut self, who: u32, day: i32, cap: usize) {
        if let Some(slot) = self.remember(who, true, day, cap) {
            self.bonds[slot].kin = true;
        }
    }

    /// Bonds that are not a fact of birth. This is what the memory cap is a
    /// cap on: family is kept whatever it costs.
    pub fn met_count(&self) -> usize {
        self.bonds.iter().filter(|b| !b.kin).count()
    }

    pub fn skill_in(&self, prof: Profession) -> f64 {
        self.skills[prof.index()] as f64
    }

    /// Practice pays into the trade being practiced, and a diligent settler
    /// learns faster. Skill is what separates a veteran crafter from a laborer
    /// standing at the same bench.
    pub fn practice(&mut self, prof: Profession, dt: f64) {
        let i = prof.index();
        let gain = dt * 0.002 * (0.6 + self.traits.diligence);
        self.skills[i] = (self.skills[i] + gain as f32).min(2.5);
        self.skill = self.skills[i] as f64;
    }

    pub fn earn(&mut self, coin: f64) {
        self.coin += coin;
        self.wage += coin;
        if self.coin > self.peak_coin {
            self.peak_coin = self.coin;
        }
    }

    pub fn spend(&mut self, coin: f64) -> f64 {
        let paid = self.coin.min(coin).max(0.0);
        self.coin -= paid;
        paid
    }

    /// Advances along the current path. Returns true once the end is reached,
    /// which is also the answer when there is no path at all.
    pub fn move_along(&mut self, dt: f64, speed: f64) -> bool {
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
        self.bob += speed * dt * 6.0;
        self.path.is_empty()
    }

    /// Needs drift whether or not the person has anything to do.
    pub fn tick_needs(&mut self, dt: f64, cfg: &PeopleConfig, working: bool) {
        let effort = if working { 1.3 } else { 1.0 };
        self.hunger = clamp01(self.hunger + cfg.hunger_rate * dt * effort);
        if self.sleeping {
            // A bed under a roof rests better than a doorstep.
            let shelter = if self.indoors() { 1.35 } else { 1.0 };
            self.energy = clamp01(self.energy + cfg.sleep_rate * dt * shelter);
        } else {
            let drain = if working { 1.6 } else { 0.6 };
            self.energy = clamp01(self.energy - cfg.tire_rate * dt * drain);
        }
        let tough = 0.6 + (1.0 - self.traits.hardiness) * 0.8;
        if self.hunger >= 1.0 {
            self.health = clamp01(self.health - cfg.starve_damage * dt * tough);
        } else if self.hunger < 0.6 {
            self.health = clamp01(self.health + cfg.heal_rate * dt);
        }
        let owned = if self.owns != 0 { 0.15 } else { 0.0 };
        // Company counts: `regard` is the standing balance of friends against
        // rivals, refreshed by the social pass rather than read from the bonds
        // here.
        let comfort = if self.home != 0 { 0.5 } else { 0.0 }
            + owned
            + self.regard
            + (1.0 - self.hunger) * 0.3
            + self.energy * 0.2;
        self.happiness = clamp01(self.happiness + (comfort - self.happiness) * clamp(dt * 0.1, 0.0, 1.0));
    }

    pub fn eat(&mut self, meal_size: f64) {
        self.hunger = clamp01(self.hunger - meal_size * 0.4);
    }

    pub fn pick(&mut self, res: Res, n: f64) -> f64 {
        if let Some(have) = self.carry.res {
            if have != res {
                return 0.0;
            }
        }
        self.carry.res = Some(res);
        self.carry.n += n;
        n
    }

    pub fn drop_load(&mut self) -> (Option<Res>, f64) {
        let out = (self.carry.res, self.carry.n);
        self.carry.res = None;
        self.carry.n = 0.0;
        out
    }
}

pub fn carry_limit(cfg: &PeopleConfig, mods: &Mods) -> f64 {
    (cfg.carry_capacity * mods.carry).round().max(1.0)
}

/// Day fraction in [0,1): 0 is midnight, 0.5 midday.
pub fn day_fraction(time: f64, cfg: &PeopleConfig) -> f64 {
    let len = cfg.day_length.max(1.0);
    (time % len) / len
}

pub fn day_number(time: f64, cfg: &PeopleConfig) -> i32 {
    (time / cfg.day_length.max(1.0)).floor() as i32
}

pub fn is_work_time(time: f64, cfg: &PeopleConfig) -> bool {
    let f = day_fraction(time, cfg);
    f >= cfg.work_start && f < cfg.work_end
}

/// Daylight in [0,1], used for the sky tint and for how well people work.
pub fn daylight(time: f64, cfg: &PeopleConfig) -> f64 {
    let f = day_fraction(time, cfg);
    clamp01((std::f64::consts::PI * clamp01((f - 0.12) / 0.76)).sin() * 1.15)
}
