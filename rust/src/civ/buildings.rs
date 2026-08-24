//! Building catalog.
//!
//! A building type declares what it costs, how much work it takes to raise,
//! how many people can work in it and what that work does. Everything else
//! (the build planner, the job scheduler, the sprite generator and the build
//! panel) is driven from these fields, so a new type only has to be added here.
//!
//! Sizes are in grid cells; wall and roof heights are in cell widths, so a
//! building keeps its proportions when the cell size of the world changes.

use serde::{Deserialize, Serialize};

use crate::civ::resources::Res;
use crate::civ::terrain::DepositKind;
use crate::species::SizeClass;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Home,
    Store,
    Gather,
    Craft,
    Civic,
    Defense,
}

pub const CATEGORIES: [Category; 6] = [
    Category::Home,
    Category::Store,
    Category::Gather,
    Category::Craft,
    Category::Civic,
    Category::Defense,
];

impl Category {
    pub fn id(self) -> &'static str {
        match self {
            Category::Home => "home",
            Category::Store => "store",
            Category::Gather => "gather",
            Category::Craft => "craft",
            Category::Civic => "civic",
            Category::Defense => "defense",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Home => "Homes",
            Category::Store => "Storage",
            Category::Gather => "Gathering",
            Category::Craft => "Workshops",
            Category::Civic => "Civic",
            Category::Defense => "Walls",
        }
    }
}

/// What a piece of the settlement physically is.
///
/// Almost everything is a roofed building; the exceptions all differ from one
/// in the same three ways, so one field decides all of them: whether the
/// planner may site it on its own, whether it stands flush against its
/// neighbors, and whether people can walk over the ground it occupies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Structure {
    /// Walls under a roof, with a door and windows.
    Building,
    /// A blank length of wall. Nothing walks through it.
    Wall,
    /// The way through a wall. Once it is standing, people walk over it.
    Gate,
    /// A counter under an awning, kept by one settler for their own account.
    Stall,
}

impl Structure {
    /// Part of a ring rather than a site of its own: placed by the wall
    /// planner, flush against its neighbors, and never chosen by `plan_next`.
    pub fn perimeter(self) -> bool {
        matches!(self, Structure::Wall | Structure::Gate)
    }

    /// Whether the ground it stands on stays walkable once it is finished. A
    /// gate is the only thing that both claims a cell and lets people cross it.
    pub fn passable(self) -> bool {
        matches!(self, Structure::Gate)
    }

    /// Stands flush against whatever is beside it, ignoring the gap the
    /// planner keeps between ordinary buildings.
    pub fn abuts(self) -> bool {
        self.perimeter()
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Job {
    Harvest {
        classes: &'static [SizeClass],
        yields: &'static [(Res, f64)],
        /// Ground cover is cut back rather than pulled up, so a patch that is
        /// picked keeps growing back. This is what makes foraging renewable and
        /// ties the food supply to the growth rates set in the species panel.
        regrow: f64,
    },
    Mine {
        deposit: DepositKind,
        yields: &'static [(Res, f64)],
    },
    Farm {
        yields: &'static [(Res, f64)],
    },
    Craft {
        input: &'static [(Res, f64)],
        output: &'static [(Res, f64)],
        time: f64,
    },
    Research,
    Trade,
    /// An inn turns food and a roof into coin and a bed for anyone without
    /// one. It is the only job that serves people rather than materials.
    Innkeep,
    /// A dock: crews load boats here and sail them to the other colonies.
    Ferry,
    /// A stall: its keeper buys stock out of the town store with their own
    /// coin and sells it over the counter to whoever walks past.
    Sell,
}

impl Job {
    /// What the job puts into the world, whether it is dug up or made.
    pub fn produces(&self) -> &'static [(Res, f64)] {
        match self {
            Job::Harvest { yields, .. } => yields,
            Job::Mine { yields, .. } => yields,
            Job::Farm { yields } => yields,
            Job::Craft { output, .. } => output,
            _ => &[],
        }
    }

    pub fn consumes(&self) -> &'static [(Res, f64)] {
        match self {
            Job::Craft { input, .. } => input,
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Site {
    Deposit(DepositKind),
    Fertile,
    /// Dry ground with navigable water within reach, which is what a dock
    /// needs and what a river gives a landlocked colony.
    Shore,
}

/// How the face of a wall reads. Only walls and gates use it, and it is the
/// one thing that tells split trunks from coursed stone once both are drawn
/// out of whatever sampling boxes the lab happens to hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grain {
    /// Split lengths standing on end: palings, posts, planking.
    Upright,
    /// Laid courses: stone, brick, turf.
    Courses,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub wall: &'static str,
    pub roof: &'static str,
    pub trim: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct BuildingDef {
    pub id: &'static str,
    pub label: &'static str,
    pub category: Category,
    /// What this is, physically. Drives placement, pathing and drawing.
    pub structure: Structure,
    pub w: i32,
    pub h: i32,
    pub wall_h: f64,
    pub roof_h: f64,
    pub palette: Palette,
    pub cost: &'static [(Res, f64)],
    pub work: f64,
    pub housing: i32,
    pub comfort: f64,
    pub storage: f64,
    pub is_store: bool,
    pub keeps_food: bool,
    pub slots: usize,
    pub radius: f64,
    pub site: Option<Site>,
    pub fields: i32,
    pub job: Option<Job>,
    pub is_market: bool,
    pub health: f64,
    pub smoke: i32,
    pub base: bool,
    /// The next rung of a home. A settler who owns this and has the coin for it
    /// commissions the upgrade themselves; the planner never does.
    pub upgrade_to: Option<&'static str>,
    /// Personal coin the owner has to put up to commission that upgrade. It is
    /// paid into the colony treasury, which is what then funds the materials.
    pub upgrade_coin: f64,
    pub grain: Grain,
    /// People who belong here stand inside it rather than in front of it.
    pub indoor: bool,
    /// Beds for hire, for settlers with no house of their own.
    pub rooms: i32,
    pub is_inn: bool,
    /// Boats moor here, and it has to be built within reach of water.
    pub is_dock: bool,
    /// How much owning this lifts the owner's standing in the colony.
    pub prestige: f64,
    /// Whether the build planner may choose this on its own. A tower is raised
    /// by one rich settler, never by the town.
    pub planned: bool,
    /// What a stall keeper may put on the counter, in the order they try to
    /// buy it. Empty for everything that is not a stall.
    pub sells: &'static [Res],
    /// Personal coin a settler puts up to open one of these for themselves.
    /// It is paid into the treasury, which is what then funds the materials.
    pub keeper_coin: f64,
    pub note: Option<&'static str>,
}

const BLANK: BuildingDef = BuildingDef {
    id: "",
    label: "",
    category: Category::Civic,
    structure: Structure::Building,
    w: 1,
    h: 1,
    wall_h: 0.6,
    roof_h: 0.3,
    palette: Palette { wall: "mat-timber", roof: "mat-thatch", trim: "mat-trunk" },
    cost: &[],
    work: 10.0,
    housing: 0,
    comfort: 0.0,
    storage: 0.0,
    is_store: false,
    keeps_food: false,
    slots: 0,
    radius: 0.0,
    site: None,
    fields: 0,
    job: None,
    is_market: false,
    health: 0.0,
    smoke: 0,
    base: false,
    upgrade_to: None,
    upgrade_coin: 0.0,
    grain: Grain::Upright,
    indoor: false,
    rooms: 0,
    is_inn: false,
    is_dock: false,
    prestige: 0.0,
    planned: true,
    sells: &[],
    keeper_coin: 0.0,
    note: None,
};

pub static BUILDINGS: &[BuildingDef] = &[
    BuildingDef {
        id: "hut",
        label: "Hut",
        category: Category::Home,
        wall_h: 0.6,
        roof_h: 0.35,
        cost: &[(Res::Wood, 8.0), (Res::Fiber, 5.0)],
        work: 18.0,
        housing: 3,
        comfort: 0.4,
        base: true,
        indoor: true,
        upgrade_to: Some("house"),
        upgrade_coin: 90.0,
        note: Some("Poles and thatch. The first shelter anyone builds."),
        ..BLANK
    },
    BuildingDef {
        id: "house",
        label: "House",
        category: Category::Home,
        w: 2,
        wall_h: 0.9,
        roof_h: 0.45,
        palette: Palette { wall: "mat-timber", roof: "mat-thatch", trim: "mat-stone" },
        cost: &[(Res::Plank, 10.0), (Res::Stone, 6.0)],
        work: 45.0,
        housing: 5,
        comfort: 0.75,
        indoor: true,
        prestige: 0.2,
        upgrade_to: Some("manor"),
        upgrade_coin: 260.0,
        note: Some("A framed house on a stone footing."),
        ..BLANK
    },
    BuildingDef {
        id: "manor",
        label: "Manor",
        category: Category::Home,
        w: 2,
        h: 2,
        wall_h: 1.26,
        roof_h: 0.55,
        palette: Palette { wall: "mat-brick", roof: "mat-thatch", trim: "mat-stone" },
        cost: &[(Res::Brick, 18.0), (Res::Plank, 14.0), (Res::Cloth, 4.0)],
        work: 120.0,
        housing: 10,
        comfort: 1.0,
        indoor: true,
        prestige: 0.6,
        upgrade_to: Some("tower"),
        upgrade_coin: 700.0,
        note: Some("Brick walls, glazed windows, room for a large household."),
        ..BLANK
    },
    BuildingDef {
        id: "tower",
        label: "Tower",
        category: Category::Home,
        w: 2,
        h: 2,
        wall_h: 3.4,
        roof_h: 0.9,
        palette: Palette { wall: "mat-stone", roof: "mat-plank", trim: "mat-metal" },
        cost: &[(Res::Brick, 44.0), (Res::Stone, 30.0), (Res::Plank, 18.0), (Res::Metal, 8.0)],
        work: 340.0,
        housing: 8,
        comfort: 1.4,
        indoor: true,
        prestige: 1.5,
        // Nobody plans a tower. One settler who has made more coin than they
        // can spend has one raised over their own manor, and the rest of the
        // town gets a landmark out of it.
        planned: false,
        note: Some("A stone tower over the roofs. The mark of a fortune, not of a plan."),
        ..BLANK
    },
    BuildingDef {
        id: "storehouse",
        label: "Storehouse",
        category: Category::Store,
        w: 2,
        wall_h: 0.78,
        roof_h: 0.4,
        cost: &[(Res::Wood, 14.0)],
        work: 30.0,
        storage: 500.0,
        is_store: true,
        base: true,
        note: Some("Where everything gathered is dropped off and drawn from."),
        ..BLANK
    },
    BuildingDef {
        id: "granary",
        label: "Granary",
        category: Category::Store,
        w: 2,
        wall_h: 0.96,
        roof_h: 0.5,
        palette: Palette { wall: "mat-plank", roof: "mat-thatch", trim: "mat-brick" },
        cost: &[(Res::Plank, 10.0), (Res::Brick, 6.0)],
        work: 55.0,
        storage: 400.0,
        is_store: true,
        keeps_food: true,
        note: Some("Raised and dry: food keeps far longer here."),
        ..BLANK
    },
    BuildingDef {
        id: "woodcutter",
        label: "Woodcutter camp",
        category: Category::Gather,
        wall_h: 0.54,
        roof_h: 0.3,
        palette: Palette { wall: "mat-trunk", roof: "mat-thatch", trim: "mat-timber" },
        cost: &[(Res::Wood, 6.0)],
        work: 14.0,
        slots: 2,
        radius: 16.0,
        job: Some(Job::Harvest {
            classes: &[SizeClass::Tree, SizeClass::Shrub],
            yields: &[(Res::Wood, 1.0), (Res::Fiber, 0.15)],
            regrow: 0.0,
        }),
        base: true,
        note: Some("Fells grown trees and shrubs for timber. The forest regrows."),
        ..BLANK
    },
    BuildingDef {
        id: "forager",
        label: "Forager hut",
        category: Category::Gather,
        wall_h: 0.48,
        roof_h: 0.3,
        palette: Palette { wall: "mat-timber", roof: "mat-thatch", trim: "mat-stem" },
        cost: &[(Res::Wood, 4.0), (Res::Fiber, 4.0)],
        work: 12.0,
        slots: 2,
        radius: 18.0,
        job: Some(Job::Harvest {
            classes: &[SizeClass::Ground, SizeClass::Herb, SizeClass::Vine],
            yields: &[(Res::Food, 1.0), (Res::Fiber, 0.4)],
            regrow: 0.35,
        }),
        base: true,
        note: Some("Picks the low growth for food and fiber; mats are cut back, not pulled up."),
        ..BLANK
    },
    BuildingDef {
        id: "quarry",
        label: "Quarry",
        category: Category::Gather,
        w: 2,
        wall_h: 0.36,
        roof_h: 0.2,
        palette: Palette { wall: "mat-stone", roof: "mat-stone", trim: "mat-timber" },
        cost: &[(Res::Wood, 10.0)],
        work: 32.0,
        slots: 3,
        radius: 14.0,
        site: Some(Site::Deposit(DepositKind::Stone)),
        job: Some(Job::Mine { deposit: DepositKind::Stone, yields: &[(Res::Stone, 1.0)] }),
        note: Some("Works a stone deposit until it is spent."),
        ..BLANK
    },
    BuildingDef {
        id: "claypit",
        label: "Clay pit",
        category: Category::Gather,
        wall_h: 0.3,
        roof_h: 0.15,
        palette: Palette { wall: "mat-soil", roof: "mat-soil", trim: "mat-timber" },
        cost: &[(Res::Wood, 8.0)],
        work: 24.0,
        slots: 2,
        radius: 14.0,
        site: Some(Site::Deposit(DepositKind::Clay)),
        job: Some(Job::Mine { deposit: DepositKind::Clay, yields: &[(Res::Clay, 1.0)] }),
        note: Some("Digs the clay banks near water."),
        ..BLANK
    },
    BuildingDef {
        id: "mine",
        label: "Mine",
        category: Category::Gather,
        w: 2,
        h: 2,
        wall_h: 0.72,
        roof_h: 0.35,
        palette: Palette { wall: "mat-timber", roof: "mat-stone", trim: "mat-metal" },
        cost: &[(Res::Plank, 12.0), (Res::Stone, 10.0)],
        work: 90.0,
        slots: 3,
        radius: 16.0,
        site: Some(Site::Deposit(DepositKind::Ore)),
        job: Some(Job::Mine { deposit: DepositKind::Ore, yields: &[(Res::Ore, 1.0)] }),
        note: Some("Props and a shaft down to the ore."),
        ..BLANK
    },
    BuildingDef {
        id: "farm",
        label: "Farm",
        category: Category::Gather,
        w: 2,
        h: 2,
        wall_h: 0.54,
        roof_h: 0.3,
        palette: Palette { wall: "mat-timber", roof: "mat-thatch", trim: "mat-stem" },
        cost: &[(Res::Wood, 8.0), (Res::Fiber, 4.0)],
        work: 40.0,
        slots: 3,
        radius: 3.0,
        site: Some(Site::Fertile),
        fields: 2,
        job: Some(Job::Farm { yields: &[(Res::Food, 1.0)] }),
        note: Some("Sown fields around the barn; yield follows soil fertility."),
        ..BLANK
    },
    BuildingDef {
        id: "sawpit",
        label: "Sawpit",
        category: Category::Craft,
        w: 2,
        wall_h: 0.54,
        roof_h: 0.25,
        cost: &[(Res::Wood, 10.0), (Res::Stone, 4.0)],
        work: 40.0,
        slots: 2,
        job: Some(Job::Craft { input: &[(Res::Wood, 2.0)], output: &[(Res::Plank, 1.0)], time: 3.0 }),
        ..BLANK
    },
    BuildingDef {
        id: "charcoalHearth",
        label: "Charcoal hearth",
        category: Category::Craft,
        wall_h: 0.48,
        roof_h: 0.25,
        palette: Palette { wall: "mat-stone", roof: "mat-soil", trim: "mat-trunk" },
        cost: &[(Res::Stone, 8.0), (Res::Wood, 4.0)],
        work: 26.0,
        slots: 1,
        smoke: 1,
        job: Some(Job::Craft { input: &[(Res::Wood, 3.0)], output: &[(Res::Charcoal, 1.0)], time: 4.0 }),
        ..BLANK
    },
    BuildingDef {
        id: "kiln",
        label: "Kiln",
        category: Category::Craft,
        wall_h: 0.72,
        roof_h: 0.25,
        palette: Palette { wall: "mat-brick", roof: "mat-stone", trim: "mat-soil" },
        cost: &[(Res::Stone, 10.0), (Res::Clay, 6.0)],
        work: 45.0,
        slots: 2,
        smoke: 1,
        job: Some(Job::Craft {
            input: &[(Res::Clay, 2.0), (Res::Charcoal, 1.0)],
            output: &[(Res::Brick, 2.0)],
            time: 4.0,
        }),
        ..BLANK
    },
    BuildingDef {
        id: "weaver",
        label: "Weaver",
        category: Category::Craft,
        wall_h: 0.72,
        roof_h: 0.35,
        palette: Palette { wall: "mat-plank", roof: "mat-cloth", trim: "mat-timber" },
        cost: &[(Res::Plank, 6.0), (Res::Wood, 4.0)],
        work: 35.0,
        slots: 2,
        job: Some(Job::Craft { input: &[(Res::Fiber, 3.0)], output: &[(Res::Cloth, 1.0)], time: 4.0 }),
        ..BLANK
    },
    BuildingDef {
        id: "smelter",
        label: "Smelter",
        category: Category::Craft,
        w: 2,
        wall_h: 0.96,
        roof_h: 0.25,
        palette: Palette { wall: "mat-brick", roof: "mat-stone", trim: "mat-metal" },
        cost: &[(Res::Brick, 12.0), (Res::Stone, 8.0)],
        work: 80.0,
        slots: 2,
        smoke: 2,
        job: Some(Job::Craft {
            input: &[(Res::Ore, 2.0), (Res::Charcoal, 2.0)],
            output: &[(Res::Metal, 1.0)],
            time: 5.0,
        }),
        ..BLANK
    },
    BuildingDef {
        id: "smithy",
        label: "Smithy",
        category: Category::Craft,
        w: 2,
        wall_h: 0.84,
        roof_h: 0.4,
        palette: Palette { wall: "mat-brick", roof: "mat-thatch", trim: "mat-metal" },
        cost: &[(Res::Brick, 10.0), (Res::Plank, 8.0)],
        work: 85.0,
        slots: 2,
        smoke: 1,
        job: Some(Job::Craft {
            input: &[(Res::Metal, 1.0), (Res::Charcoal, 1.0)],
            output: &[(Res::Tool, 1.0)],
            time: 6.0,
        }),
        ..BLANK
    },
    BuildingDef {
        id: "workshop",
        label: "Workshop",
        category: Category::Craft,
        w: 2,
        h: 2,
        wall_h: 1.08,
        roof_h: 0.45,
        palette: Palette { wall: "mat-brick", roof: "mat-plank", trim: "mat-metal" },
        cost: &[(Res::Brick, 14.0), (Res::Plank, 12.0), (Res::Tool, 2.0)],
        work: 140.0,
        slots: 4,
        job: Some(Job::Craft {
            input: &[(Res::Metal, 2.0), (Res::Plank, 2.0)],
            output: &[(Res::Tool, 3.0)],
            time: 5.0,
        }),
        note: Some("Machines and jigs: three tools for the metal that made one."),
        ..BLANK
    },
    BuildingDef {
        id: "school",
        label: "School",
        category: Category::Civic,
        w: 2,
        wall_h: 1.02,
        roof_h: 0.5,
        palette: Palette { wall: "mat-plank", roof: "mat-thatch", trim: "mat-brick" },
        cost: &[(Res::Plank, 12.0), (Res::Brick, 6.0)],
        work: 70.0,
        slots: 3,
        job: Some(Job::Research),
        note: Some("Scholars turn a fed population into research points."),
        ..BLANK
    },
    BuildingDef {
        id: "market",
        label: "Market",
        category: Category::Civic,
        w: 2,
        h: 2,
        wall_h: 0.66,
        roof_h: 0.3,
        palette: Palette { wall: "mat-cloth", roof: "mat-cloth", trim: "mat-timber" },
        cost: &[(Res::Plank, 14.0), (Res::Cloth, 4.0)],
        work: 90.0,
        slots: 2,
        job: Some(Job::Trade),
        is_market: true,
        note: Some("Sets prices and lets caravans buy the surplus."),
        ..BLANK
    },
    BuildingDef {
        id: "well",
        label: "Well",
        category: Category::Civic,
        wall_h: 0.42,
        roof_h: 0.25,
        palette: Palette { wall: "mat-stone", roof: "mat-timber", trim: "mat-metal" },
        cost: &[(Res::Stone, 12.0)],
        work: 40.0,
        health: 0.35,
        radius: 12.0,
        note: Some("Clean water: fewer deaths, faster recovery."),
        ..BLANK
    },
    BuildingDef {
        id: "inn",
        label: "Inn",
        category: Category::Civic,
        w: 2,
        wall_h: 1.14,
        roof_h: 0.5,
        palette: Palette { wall: "mat-plank", roof: "mat-thatch", trim: "mat-timber" },
        cost: &[(Res::Plank, 12.0), (Res::Wood, 6.0), (Res::Cloth, 3.0)],
        work: 65.0,
        slots: 1,
        rooms: 5,
        is_inn: true,
        indoor: true,
        comfort: 0.5,
        smoke: 1,
        radius: 26.0,
        job: Some(Job::Innkeep),
        note: Some("Beds for hire and a hot meal. Where anyone without a roof sleeps."),
        ..BLANK
    },
    BuildingDef {
        id: "dock",
        label: "Dock",
        category: Category::Civic,
        w: 2,
        wall_h: 0.42,
        roof_h: 0.2,
        palette: Palette { wall: "mat-plank", roof: "mat-timber", trim: "mat-trunk" },
        cost: &[(Res::Plank, 10.0), (Res::Wood, 8.0)],
        work: 55.0,
        slots: 2,
        storage: 80.0,
        site: Some(Site::Shore),
        radius: 3.0,
        is_dock: true,
        job: Some(Job::Ferry),
        note: Some("A jetty out over the water. Boats are built and loaded here."),
        ..BLANK
    },
    BuildingDef {
        id: "stall",
        label: "Market stall",
        category: Category::Civic,
        structure: Structure::Stall,
        wall_h: 0.5,
        roof_h: 0.42,
        palette: Palette { wall: "mat-cloth", roof: "mat-cloth", trim: "mat-timber" },
        cost: &[(Res::Wood, 5.0), (Res::Cloth, 2.0)],
        work: 20.0,
        slots: 1,
        radius: 14.0,
        job: Some(Job::Sell),
        sells: &[Res::Food, Res::Cloth, Res::Tool],
        keeper_coin: 40.0,
        prestige: 0.15,
        // A stall is one settler's idea, not the town's. Nobody plans one and
        // nobody is assigned to keep one: somebody buys it and stands in it.
        planned: false,
        note: Some("A counter and an awning. Its keeper buys stock out of the store \
                    with their own coin and sells it to whoever walks past."),
        ..BLANK
    },
    BuildingDef {
        id: "palisade",
        label: "Palisade",
        category: Category::Defense,
        structure: Structure::Wall,
        wall_h: 1.05,
        roof_h: 0.12,
        palette: Palette { wall: "mat-trunk", roof: "mat-timber", trim: "mat-timber" },
        cost: &[(Res::Wood, 5.0)],
        work: 14.0,
        planned: false,
        note: Some("Split trunks driven into the ground, one cell at a time around the town."),
        ..BLANK
    },
    BuildingDef {
        id: "rampart",
        label: "Rampart",
        category: Category::Defense,
        structure: Structure::Wall,
        wall_h: 1.5,
        roof_h: 0.16,
        palette: Palette { wall: "mat-stone", roof: "mat-stone", trim: "mat-brick" },
        grain: Grain::Courses,
        cost: &[(Res::Stone, 6.0), (Res::Brick, 2.0)],
        work: 34.0,
        prestige: 0.05,
        planned: false,
        note: Some("Coursed stone. Raised on the same ring, wherever the palisade has not reached."),
        ..BLANK
    },
    BuildingDef {
        id: "gate",
        label: "Gate",
        category: Category::Defense,
        structure: Structure::Gate,
        wall_h: 1.35,
        roof_h: 0.3,
        palette: Palette { wall: "mat-timber", roof: "mat-plank", trim: "mat-metal" },
        cost: &[(Res::Plank, 6.0), (Res::Wood, 4.0)],
        work: 26.0,
        prestige: 0.05,
        planned: false,
        note: Some("The way through. Gates are cut where the paths already run, \
                    and the ring is only ever closed around them."),
        ..BLANK
    },
];

pub fn building_by_id(id: &str) -> Option<&'static BuildingDef> {
    BUILDINGS.iter().find(|b| b.id == id)
}

/// The home one rung up from this one, if there is one.
pub fn upgrade_of(def: &BuildingDef) -> Option<&'static BuildingDef> {
    def.upgrade_to.and_then(building_by_id)
}

/// How far up the ladder of homes a building sits, counting from the hut.
pub fn home_rank(def: &BuildingDef) -> i32 {
    let mut rank = 0;
    let mut cur = building_by_id("hut");
    while let Some(step) = cur {
        if step.id == def.id {
            return rank;
        }
        rank += 1;
        cur = upgrade_of(step);
    }
    rank
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoryWeights {
    pub home: f64,
    pub store: f64,
    pub gather: f64,
    pub craft: f64,
    pub civic: f64,
    pub defense: f64,
}

impl Default for CategoryWeights {
    fn default() -> Self {
        CategoryWeights { home: 1.0, store: 1.0, gather: 1.0, craft: 1.0, civic: 0.8, defense: 0.7 }
    }
}

impl CategoryWeights {
    pub fn get(&self, cat: Category) -> f64 {
        match cat {
            Category::Home => self.home,
            Category::Store => self.store,
            Category::Gather => self.gather,
            Category::Craft => self.craft,
            Category::Civic => self.civic,
            Category::Defense => self.defense,
        }
    }

    pub fn get_mut(&mut self, cat: Category) -> &mut f64 {
        match cat {
            Category::Home => &mut self.home,
            Category::Store => &mut self.store,
            Category::Gather => &mut self.gather,
            Category::Craft => &mut self.craft,
            Category::Civic => &mut self.civic,
            Category::Defense => &mut self.defense,
        }
    }
}

/// People per building of a category before a second one is worth raising.
/// Homes are left out: how many are needed follows from the housing need.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PerType {
    pub store: i32,
    pub gather: i32,
    pub craft: i32,
    pub civic: i32,
}

impl Default for PerType {
    fn default() -> Self {
        PerType { store: 12, gather: 4, craft: 8, civic: 14 }
    }
}

impl PerType {
    pub fn get(&self, cat: Category) -> Option<i32> {
        match cat {
            Category::Home => None,
            Category::Store => Some(self.store),
            Category::Gather => Some(self.gather),
            Category::Craft => Some(self.craft),
            Category::Civic => Some(self.civic),
            // A ring is as long as it is; head count has nothing to do with it.
            Category::Defense => None,
        }
    }

    pub fn get_mut(&mut self, cat: Category) -> Option<&mut i32> {
        match cat {
            Category::Home => None,
            Category::Store => Some(&mut self.store),
            Category::Gather => Some(&mut self.gather),
            Category::Craft => Some(&mut self.craft),
            Category::Civic => Some(&mut self.civic),
            Category::Defense => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BuildConfig {
    pub cost_scale: f64,
    pub work_scale: f64,
    pub auto_build: bool,
    pub max_sites: i32,
    pub spacing: i32,
    /// How far a new building may be placed from the center of the settlement.
    pub sprawl: i32,
    /// Planner weights: how badly the settlement wants each category.
    pub weights: CategoryWeights,
    /// Housing headroom kept ahead of the population, in people.
    pub housing_slack: i32,
    pub per_type: PerType,
    /// Whether a settler with the coin for it may have their own house rebuilt
    /// one rung larger.
    pub home_upgrades: bool,
    /// Multiplier on the coin an owner has to put up for that.
    pub upgrade_scale: f64,
    /// Share of the old house's materials that count toward the new one.
    pub upgrade_salvage: f64,
    /// Homes a town may have on the ground at once. A rebuild takes its beds
    /// out of the housing stock, so a town that starts them all at once stops
    /// having children and then starves.
    pub max_home_rebuilds: i32,
    /// Whether a crowded colony sends settlers out to found another.
    pub expeditions: bool,
    pub max_colonies: i32,
    /// Simulated seconds between one colony's attempts to send a party out.
    pub expedition_interval: f64,
    /// Head count a colony has to reach before it will spare anyone.
    pub expedition_population: i32,
    /// Adults who walk out, not counting the families that follow them.
    pub expedition_party: i32,
    /// Of each founding resource, carried out of the parent's store.
    pub expedition_supplies: f64,
    /// Cells kept between the centers of two colonies.
    pub colony_spacing: i32,
    /// Whether a town rings itself with a wall once it knows how.
    pub walls: bool,
    /// Head count a town has to reach before a ring is worth the timber. A
    /// village that walls itself spends everything it has on the wall and then
    /// starves inside it.
    pub wall_population: i32,
    /// Cells left between the outermost building and the ring, so the ring
    /// stands clear of the doors it is protecting.
    pub wall_margin: i32,
    /// Ways through a ring. The busiest cells of the ring become the gates,
    /// which is to say the gates end up on the roads people already walk.
    pub wall_gates: i32,
    /// Pieces of wall a town may have going up at once. Counted separately
    /// from `max_sites`, or a ring would stop the town building anything else.
    pub wall_sites: i32,
    /// Whether settlers with the coin for it open stalls of their own.
    pub stalls: bool,
    /// Multiplier on the coin a settler puts up to open one.
    pub stall_price_scale: f64,
    /// What a keeper adds to the town price when they sell over the counter.
    pub stall_margin: f64,
    /// Stalls one town will support before nobody else bothers opening one.
    pub stalls_per_town: i32,
    /// People a stall needs behind it to be worth keeping. A counter takes its
    /// keeper out of every other trade, which a hamlet cannot afford.
    pub stall_customers: i32,
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig {
            cost_scale: 1.0,
            work_scale: 1.0,
            auto_build: true,
            max_sites: 3,
            spacing: 1,
            sprawl: 22,
            weights: CategoryWeights::default(),
            housing_slack: 3,
            per_type: PerType::default(),
            home_upgrades: true,
            upgrade_scale: 1.0,
            upgrade_salvage: 0.35,
            max_home_rebuilds: 1,
            expeditions: true,
            max_colonies: 4,
            expedition_interval: 1800.0,
            expedition_population: 18,
            expedition_party: 4,
            expedition_supplies: 40.0,
            colony_spacing: 34,
            walls: true,
            wall_population: 24,
            wall_margin: 3,
            wall_gates: 3,
            wall_sites: 1,
            stalls: true,
            stall_price_scale: 1.0,
            stall_margin: 0.35,
            stalls_per_town: 4,
            stall_customers: 8,
        }
    }
}

pub fn scaled_cost(def: &BuildingDef, cfg: &BuildConfig) -> Vec<(Res, f64)> {
    def.cost
        .iter()
        .map(|&(res, n)| (res, (n * cfg.cost_scale).round().max(1.0)))
        .collect()
}

pub fn scaled_work(def: &BuildingDef, cfg: &BuildConfig) -> f64 {
    (def.work * cfg.work_scale).max(1.0)
}
