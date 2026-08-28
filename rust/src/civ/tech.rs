//! Technology: a small directed graph of unlocks.
//!
//! A tech costs research points, needs its prerequisites, and pays out in two
//! ways: it unlocks building types and it raises named modifiers that the rest
//! of the sim multiplies its rates by. Nothing else in the sim knows the name
//! of a tech, only the modifiers and the unlock list, so the tree can be
//! reshaped here without touching the simulation.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mod {
    Gather,
    Build,
    Craft,
    Carry,
    Farm,
    Research,
    Trade,
    Comfort,
    Yield,
}

pub const MOD_KEYS: [Mod; 9] = [
    Mod::Gather,
    Mod::Build,
    Mod::Craft,
    Mod::Carry,
    Mod::Farm,
    Mod::Research,
    Mod::Trade,
    Mod::Comfort,
    Mod::Yield,
];

impl Mod {
    pub fn label(self) -> &'static str {
        match self {
            Mod::Gather => "Gathering speed",
            Mod::Build => "Construction speed",
            Mod::Craft => "Crafting speed",
            Mod::Carry => "Carry capacity",
            Mod::Farm => "Farm yield",
            Mod::Research => "Research output",
            Mod::Trade => "Trade margin",
            Mod::Comfort => "Housing comfort",
            Mod::Yield => "Harvest yield",
        }
    }
}

/// Multipliers applied all over the sim. Effects are additive fractions, so
/// three techs worth +0.1 gathering give x1.3 rather than x1.331.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Mods {
    pub gather: f64,
    pub build: f64,
    pub craft: f64,
    pub carry: f64,
    pub farm: f64,
    pub research: f64,
    pub trade: f64,
    pub comfort: f64,
    pub yields: f64,
}

impl Default for Mods {
    fn default() -> Self {
        Mods {
            gather: 1.0,
            build: 1.0,
            craft: 1.0,
            carry: 1.0,
            farm: 1.0,
            research: 1.0,
            trade: 1.0,
            comfort: 1.0,
            yields: 1.0,
        }
    }
}

impl Mods {
    pub fn get(&self, key: Mod) -> f64 {
        match key {
            Mod::Gather => self.gather,
            Mod::Build => self.build,
            Mod::Craft => self.craft,
            Mod::Carry => self.carry,
            Mod::Farm => self.farm,
            Mod::Research => self.research,
            Mod::Trade => self.trade,
            Mod::Comfort => self.comfort,
            Mod::Yield => self.yields,
        }
    }

    fn add(&mut self, key: Mod, v: f64) {
        match key {
            Mod::Gather => self.gather += v,
            Mod::Build => self.build += v,
            Mod::Craft => self.craft += v,
            Mod::Carry => self.carry += v,
            Mod::Farm => self.farm += v,
            Mod::Research => self.research += v,
            Mod::Trade => self.trade += v,
            Mod::Comfort => self.comfort += v,
            Mod::Yield => self.yields += v,
        }
    }
}

pub struct TechDef {
    pub id: &'static str,
    pub label: &'static str,
    pub cost: f64,
    pub requires: &'static [&'static str],
    pub unlocks: &'static [&'static str],
    pub effects: &'static [(Mod, f64)],
    pub note: &'static str,
}

pub static TECHS: &[TechDef] = &[
    TechDef {
        id: "stonework",
        label: "Stone working",
        cost: 24.0,
        requires: &[],
        unlocks: &["quarry"],
        effects: &[(Mod::Gather, 0.1)],
        note: "Shaped stone: opens the quarry and speeds up gathering.",
    },
    TechDef {
        id: "firecraft",
        label: "Firecraft",
        cost: 40.0,
        requires: &["stonework"],
        unlocks: &["charcoalHearth"],
        effects: &[],
        note: "Controlled burning: charcoal for every later furnace.",
    },
    TechDef {
        id: "carpentry",
        label: "Carpentry",
        cost: 55.0,
        requires: &["stonework"],
        unlocks: &["sawpit", "house"],
        effects: &[(Mod::Build, 0.15)],
        note: "Sawn planks and framed houses.",
    },
    TechDef {
        id: "agriculture",
        label: "Agriculture",
        cost: 70.0,
        requires: &["stonework"],
        unlocks: &["farm", "granary"],
        effects: &[(Mod::Farm, 0.1)],
        note: "Sown fields instead of foraging.",
    },
    TechDef {
        id: "pottery",
        label: "Pottery",
        cost: 80.0,
        requires: &["firecraft"],
        unlocks: &["claypit", "kiln"],
        effects: &[],
        note: "Fired clay: bricks, and the vessels that keep food.",
    },
    TechDef {
        id: "weaving",
        label: "Weaving",
        cost: 90.0,
        requires: &["agriculture"],
        unlocks: &["weaver"],
        effects: &[(Mod::Comfort, 0.1)],
        note: "Cloth from fiber.",
    },
    TechDef {
        id: "cartage",
        label: "Cartage",
        cost: 110.0,
        requires: &["carpentry"],
        unlocks: &[],
        effects: &[(Mod::Carry, 0.6)],
        note: "Barrows and carts: every worker hauls far more per trip.",
    },
    TechDef {
        id: "writing",
        label: "Writing",
        cost: 130.0,
        requires: &["pottery"],
        unlocks: &["school"],
        effects: &[(Mod::Research, 0.2)],
        note: "Records that outlive the person who made them.",
    },
    TechDef {
        id: "masonry",
        label: "Masonry",
        cost: 160.0,
        requires: &["carpentry", "pottery"],
        unlocks: &["well", "manor", "rampart"],
        effects: &[(Mod::Comfort, 0.15), (Mod::Build, 0.1)],
        note: "Mortared walls, wells, larger houses and a rampart of coursed stone.",
    },
    TechDef {
        id: "mining",
        label: "Mining",
        cost: 190.0,
        requires: &["stonework", "carpentry"],
        unlocks: &["mine"],
        effects: &[(Mod::Yield, 0.1)],
        note: "Shafts and props reach the ore.",
    },
    TechDef {
        id: "trade",
        label: "Trade",
        cost: 210.0,
        requires: &["writing"],
        unlocks: &["market", "stall"],
        effects: &[(Mod::Trade, 0.2)],
        note: "A market, prices, caravans that answer them, and stalls anyone may keep.",
    },
    TechDef {
        id: "smelting",
        label: "Smelting",
        cost: 260.0,
        requires: &["mining", "firecraft"],
        unlocks: &["smelter"],
        effects: &[],
        note: "Ore and charcoal into metal.",
    },
    TechDef {
        id: "mathematics",
        label: "Mathematics",
        cost: 300.0,
        requires: &["writing"],
        unlocks: &[],
        effects: &[(Mod::Research, 0.25), (Mod::Build, 0.1)],
        note: "Measure, plan, and predict.",
    },
    TechDef {
        id: "metallurgy",
        label: "Metallurgy",
        cost: 380.0,
        requires: &["smelting"],
        unlocks: &["smithy"],
        effects: &[(Mod::Gather, 0.2), (Mod::Craft, 0.15)],
        note: "Metal tools in every hand.",
    },
    TechDef {
        id: "irrigation",
        label: "Irrigation",
        cost: 420.0,
        requires: &["agriculture", "masonry"],
        unlocks: &[],
        effects: &[(Mod::Farm, 0.45)],
        note: "Water carried to the fields.",
    },
    TechDef {
        id: "engineering",
        label: "Engineering",
        cost: 560.0,
        requires: &["mathematics", "metallurgy"],
        unlocks: &["workshop"],
        effects: &[(Mod::Build, 0.3), (Mod::Craft, 0.25)],
        note: "Machines that multiply a day of work.",
    },
    TechDef {
        id: "fortification",
        label: "Fortification",
        cost: 120.0,
        requires: &["carpentry"],
        unlocks: &["palisade", "gate"],
        effects: &[],
        note: "A ring of split trunks around the town, and gates cut where the paths run.",
    },
    TechDef {
        id: "hospitality",
        label: "Hospitality",
        cost: 150.0,
        requires: &["weaving"],
        unlocks: &["inn"],
        effects: &[(Mod::Comfort, 0.12)],
        note: "An inn: a bed for anyone without one, and coin for the room.",
    },
    TechDef {
        id: "boatbuilding",
        label: "Boat building",
        cost: 180.0,
        requires: &["carpentry"],
        unlocks: &["dock"],
        effects: &[(Mod::Trade, 0.15)],
        note: "Hulls and oars: the rivers become roads between the colonies.",
    },
    TechDef {
        id: "architecture",
        label: "Architecture",
        cost: 640.0,
        requires: &["masonry", "engineering"],
        unlocks: &["tower"],
        effects: &[(Mod::Build, 0.15), (Mod::Comfort, 0.2)],
        note: "Walls that carry their own weight upward. A tower can be raised.",
    },
    TechDef {
        id: "printing",
        label: "Printing",
        cost: 700.0,
        requires: &["engineering"],
        unlocks: &[],
        effects: &[(Mod::Research, 0.6)],
        note: "Knowledge copied faster than it is forgotten.",
    },
];

pub fn tech_by_id(id: &str) -> Option<&'static TechDef> {
    TECHS.iter().find(|t| t.id == id)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TechConfig {
    pub cost_scale: f64,
    pub research_per_scholar: f64,
    pub insight_per_person: f64,
    pub auto_research: bool,
    /// Auto research prefers the cheapest reachable tech; raising this makes it
    /// prefer techs that unlock buildings the settlement is short of.
    pub need_bias: f64,
}

impl Default for TechConfig {
    fn default() -> Self {
        TechConfig {
            cost_scale: 1.0,
            research_per_scholar: 0.6,
            insight_per_person: 0.006,
            auto_research: true,
            need_bias: 0.5,
        }
    }
}

/// What one colony has learned. The ids are owned rather than borrowed from
/// the table they name, because a settlement read back off a save has to hold
/// them before anything has looked them up.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TechState {
    pub known: Vec<String>,
    pub points: f64,
    pub spent: f64,
    pub target: Option<String>,
    pub log: Vec<(String, i32)>,
}

impl TechState {
    pub fn is_known(&self, id: &str) -> bool {
        self.known.iter().any(|k| k == id)
    }

    pub fn reachable(&self, def: &TechDef) -> bool {
        def.requires.iter().all(|r| self.is_known(r))
    }

    pub fn available(&self) -> Vec<&'static TechDef> {
        TECHS
            .iter()
            .filter(|t| !self.is_known(t.id) && self.reachable(t))
            .collect()
    }

    pub fn locked(&self) -> Vec<&'static TechDef> {
        TECHS
            .iter()
            .filter(|t| !self.is_known(t.id) && !self.reachable(t))
            .collect()
    }

    pub fn modifiers(&self) -> Mods {
        let mut mods = Mods::default();
        for id in &self.known {
            if let Some(def) = tech_by_id(id) {
                for &(key, v) in def.effects {
                    mods.add(key, v);
                }
            }
        }
        mods
    }

    pub fn unlocked_buildings(&self) -> HashSet<&'static str> {
        let mut set = HashSet::new();
        for id in &self.known {
            if let Some(def) = tech_by_id(id) {
                for b in def.unlocks {
                    set.insert(*b);
                }
            }
        }
        set
    }
}

pub fn tech_cost(def: &TechDef, cfg: &TechConfig) -> f64 {
    (def.cost * cfg.cost_scale).round().max(1.0)
}
