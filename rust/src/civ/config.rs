//! Every knob the settlement runs on, in one place.
//!
//! The sections mirror the panels: land (map and terrain), people, work rates,
//! building and planning, economy, technology. Nothing in the simulation reads
//! a constant that is not reachable from here.

use serde::{Deserialize, Serialize};

use crate::civ::balloons::BalloonConfig;
use crate::civ::boats::BoatConfig;
use crate::civ::buildings::{BuildConfig, Category};
use crate::civ::economy::EconomyConfig;
use crate::civ::people::PeopleConfig;
use crate::civ::resources::{stock_map, Res, Stock, RES_COUNT};
use crate::civ::social::SocialConfig;
use crate::civ::sprites::{MadeSprites, PeopleSprites};
use crate::civ::tech::TechConfig;
use crate::civ::terrain::TerrainConfig;
use crate::util::clamp01;
use crate::world::WorldConfig;

pub fn default_civ_world() -> WorldConfig {
    WorldConfig {
        cols: 128,
        rows: 52,
        cell_px: 8,
        depth_px: 5,
        sky_px: 110,
        depth_fade: 0.14,
        ..WorldConfig::default()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkConfig {
    pub harvest_rate: f64,
    pub mine_rate: f64,
    pub build_rate: f64,
    pub craft_rate: f64,
    pub farm_rate: f64,
    /// A plant has to be worth the walk before anyone fells it.
    pub min_harvest_mass: f64,
    pub clear_yield: f64,
    /// What a felled plant leaves on the ground rots away over roughly this
    /// many days if nobody comes back for it. A week by default: long enough
    /// that a town with other work on gets to a cut eventually, short enough
    /// that what nobody ever wanted does not lie there for good.
    pub pile_life: f64,
    /// Fraction of a full load a hauler is willing to fetch for a workshop.
    pub restock_share: f64,
    /// How far somebody will walk for a load lying on the ground, in cells.
    /// Past it they leave it where it is, unless there is nothing nearer to
    /// fetch at all, in which case the nearest one beyond reach is better than
    /// standing about. A load that was cut by hand is worth half again as long
    /// a walk, because somebody asked for that one.
    pub fetch_reach: f64,
    /// Water a farm uses per second of work. Fields dry out as they are
    /// worked, and a dry field is a poor one.
    pub farm_water_use: f64,
    /// How far a field draws water from a river or a lake on its own. Inside
    /// this, the ground stays damp and nobody has to carry anything.
    pub farm_soak_reach: i32,
    /// How fast damp ground fills a farm back up, per second, at its best.
    pub farm_soak_rate: f64,
    /// What one trip with a bucket adds, as a share of a full field.
    pub farm_bucket: f64,
    /// The share of its yield a bone dry farm still brings in. Never nothing:
    /// a field with no water is poor, not barren.
    pub farm_dry_yield: f64,
    /// A farm below this asks for a bucket rather than working the field.
    pub farm_thirsty: f64,
    /// How long a cut plant takes to go over. It is off the map at the end of
    /// it, so this is also how long the ground it stood on shows a tree lying
    /// across it.
    pub fall_time: f64,
    pub plan_interval: f64,
    /// Simulated seconds between rebuilds of the coarse plant index every
    /// gathering decision reads. Higher is cheaper and staler.
    pub plant_index_interval: f64,
}

impl Default for WorkConfig {
    fn default() -> Self {
        WorkConfig {
            harvest_rate: 2.5,
            mine_rate: 1.6,
            build_rate: 1.2,
            craft_rate: 1.0,
            farm_rate: 0.6,
            min_harvest_mass: 1.5,
            clear_yield: 0.5,
            pile_life: 7.0,
            restock_share: 1.0,
            fetch_reach: 24.0,
            farm_water_use: 0.002,
            farm_soak_reach: 3,
            farm_soak_rate: 0.06,
            farm_bucket: 0.4,
            farm_dry_yield: 0.35,
            farm_thirsty: 0.35,
            fall_time: 1.2,
            plan_interval: 0.5,
            plant_index_interval: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StartConfig {
    pub population: i32,
    #[serde(with = "stock_map")]
    pub supplies: Stock,
    pub storehouse: bool,
    /// Start over on its own once nobody is left alive. Off by default: a
    /// settlement dying out is usually the thing somebody was watching for,
    /// and clearing the evidence a minute later would be no help at all.
    pub restart_when_gone: bool,
    /// Seconds of settlement time to wait before doing it, so there is a
    /// chance to look at what is left. Paused time does not count.
    pub restart_after: f64,
}

impl Default for StartConfig {
    fn default() -> Self {
        let mut supplies = [0.0; RES_COUNT];
        supplies[Res::Wood as usize] = 30.0;
        supplies[Res::Food as usize] = 24.0;
        supplies[Res::Fiber as usize] = 12.0;
        supplies[Res::Stone as usize] = 6.0;
        StartConfig {
            population: 5,
            supplies,
            storehouse: true,
            restart_when_gone: false,
            restart_after: 30.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SimSettings {
    pub speed: f64,
    pub running: bool,
    pub tick_hz: f64,
    pub raster_budget: usize,
}

impl Default for SimSettings {
    fn default() -> Self {
        SimSettings { speed: 1.0, running: true, tick_hz: 20.0, raster_budget: 12 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ViewConfig {
    pub day_night: bool,
    pub paths: bool,
    pub deposits: bool,
    pub people: bool,
    /// The master switch for every label over the map. The per category
    /// switches below hang off it, so turning names off is one click and
    /// turning them back on restores whatever was showing before.
    pub labels: bool,
    pub label_homes: bool,
    pub label_stores: bool,
    pub label_gather: bool,
    pub label_craft: bool,
    pub label_civic: bool,
    /// Walls and the gates through them, which are the labels most worth
    /// turning off on their own: a ring of palisade is a hundred of them.
    pub label_walls: bool,
    pub label_towns: bool,
    /// What foliage does where it covers a settler: `solid`, `hatched` or
    /// `faded`. Solid is what a plant is; the other two keep somebody walking
    /// through a wood findable.
    pub foliage: String,
    /// How much of the foliage is left when it is faded over a settler.
    pub foliage_alpha: f64,
    pub smoke: bool,
    /// The wind in the trees: standing plants lean from the tips, each with
    /// its own phase, and a gust that travels across the map. Runs on
    /// simulation time, so a paused world holds still and two runs of one
    /// seed stay the same picture.
    pub sway: bool,
    /// How far the crown of a full grown tree leans, in pixels. Small plants
    /// lean less by their height.
    pub sway_amp: f64,
    /// Full leans per simulated second, at speed one.
    pub sway_speed: f64,
    pub water_top: String,
    pub water_deep: String,
    pub path_color: String,
    pub boats: bool,
    /// Ripples on the rivers, which is the only thing that tells running water
    /// from a lake at a glance.
    pub current: bool,
    /// Zoom below which the drawing starts shedding detail. Raise it to keep a
    /// large map readable, lower it to keep it pretty.
    pub detail_zoom: f64,
    /// Draw only what the camera can see. Off is slower and only useful when
    /// something looks wrong at the edge of the view.
    pub cull: bool,
    /// Real seconds of nobody touching anything before the map takes the whole
    /// window on its own. Zero never does it. This is the page folding its own
    /// chrome away rather than the browser going fullscreen: a window nobody
    /// has touched cannot ask for the screen, and would not be given it.
    pub idle_fullscreen: f64,
}

/// The label switches, in the order they are shown. `None` is the town names,
/// which are not a building category but read as one in the menu.
pub const LABEL_KINDS: [(Option<Category>, &str); 7] = [
    (Some(Category::Home), "Homes"),
    (Some(Category::Store), "Stores"),
    (Some(Category::Gather), "Gathering"),
    (Some(Category::Craft), "Crafts"),
    (Some(Category::Civic), "Civic"),
    (Some(Category::Defense), "Walls and gates"),
    (None, "Town names"),
];

/// The three ways foliage can cover a settler, and what to call them.
pub const FOLIAGE_MODES: [(&str, &str); 3] = [
    ("solid", "Solid"),
    ("hatched", "Hatched"),
    ("faded", "See through"),
];

impl ViewConfig {
    /// How a plant should be drawn where it lands on a settler.
    pub fn foliage_over_people(&self) -> crate::sim::Foliage {
        match self.foliage.as_str() {
            "hatched" => crate::sim::Foliage::Hatched,
            "faded" => crate::sim::Foliage::Faded(clamp01(self.foliage_alpha)),
            _ => crate::sim::Foliage::Solid,
        }
    }

    /// Whether labels of this kind are drawn. Every kind is off while the
    /// master switch is, so one click clears the map.
    pub fn label_on(&self, kind: Option<Category>) -> bool {
        self.labels && self.label_flag(kind)
    }

    /// The kind's own switch, ignoring the master one. This is what the menu
    /// shows, so a checkbox does not appear to clear itself when labels are
    /// turned off as a whole.
    pub fn label_flag(&self, kind: Option<Category>) -> bool {
        match kind {
            Some(Category::Home) => self.label_homes,
            Some(Category::Store) => self.label_stores,
            Some(Category::Gather) => self.label_gather,
            Some(Category::Craft) => self.label_craft,
            Some(Category::Civic) => self.label_civic,
            Some(Category::Defense) => self.label_walls,
            None => self.label_towns,
        }
    }

    pub fn set_label(&mut self, kind: Option<Category>, on: bool) {
        let slot = match kind {
            Some(Category::Home) => &mut self.label_homes,
            Some(Category::Store) => &mut self.label_stores,
            Some(Category::Gather) => &mut self.label_gather,
            Some(Category::Craft) => &mut self.label_craft,
            Some(Category::Civic) => &mut self.label_civic,
            Some(Category::Defense) => &mut self.label_walls,
            None => &mut self.label_towns,
        };
        *slot = on;
    }

    /// Whether every kind is showing, which is what the All switch reads.
    pub fn all_labels(&self) -> bool {
        LABEL_KINDS.iter().all(|(kind, _)| self.label_flag(*kind))
    }
}

impl Default for ViewConfig {
    fn default() -> Self {
        ViewConfig {
            day_night: true,
            paths: true,
            deposits: true,
            people: true,
            labels: false,
            label_homes: true,
            label_stores: true,
            label_gather: true,
            label_craft: true,
            label_civic: true,
            label_walls: false,
            label_towns: true,
            foliage: "solid".into(),
            foliage_alpha: 0.5,
            smoke: true,
            sway: true,
            sway_amp: 1.6,
            sway_speed: 0.4,
            water_top: "#2b4f63".into(),
            water_deep: "#16303f".into(),
            path_color: "#6b5a44".into(),
            boats: true,
            current: true,
            detail_zoom: 1.0,
            cull: true,
            idle_fullscreen: 20.0,
        }
    }
}

/// Things that are not finished, or not sure of themselves yet.
///
/// Off by default and off as a block. Everything under here asks the switch at
/// the top first, so leaving something half thought out in it cannot change a
/// settlement somebody was watching, and turning the block off puts the world
/// back the way it ran.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Experiments {
    pub on: bool,
    pub balloons: BalloonConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CivConfig {
    pub seed: u32,
    pub world: WorldConfig,
    pub terrain: TerrainConfig,
    pub people: PeopleConfig,
    pub work: WorkConfig,
    pub build: BuildConfig,
    pub economy: EconomyConfig,
    pub social: SocialConfig,
    pub tech: TechConfig,
    pub boats: BoatConfig,
    pub start: StartConfig,
    pub sim: SimSettings,
    pub view: ViewConfig,
    pub experiments: Experiments,
    /// Images dropped on the people panel to draw settlers with, one clip per
    /// motion. Empty means everyone is drawn from the generator instead.
    pub sprites: PeopleSprites,
    /// Pictures for the things people make: buildings, walls, boats and loads
    /// in hand. Empty means everything is generated from the sampling boxes.
    pub made: MadeSprites,
    /// How many dead settlers stay on file. The register keeps a slot per
    /// person ever born; this is where a very long run stops growing.
    pub people_archive: usize,
}

impl Default for CivConfig {
    fn default() -> Self {
        CivConfig {
            seed: 77104,
            world: default_civ_world(),
            terrain: TerrainConfig::default(),
            people: PeopleConfig::default(),
            work: WorkConfig::default(),
            build: BuildConfig::default(),
            economy: EconomyConfig::default(),
            social: SocialConfig::default(),
            tech: TechConfig::default(),
            boats: BoatConfig::default(),
            start: StartConfig::default(),
            sim: SimSettings::default(),
            view: ViewConfig::default(),
            experiments: Experiments::default(),
            sprites: PeopleSprites::default(),
            made: MadeSprites::default(),
            people_archive: 400,
        }
    }
}
