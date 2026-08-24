//! Every knob the settlement runs on, in one place.
//!
//! The sections mirror the panels: land (map and terrain), people, work rates,
//! building and planning, economy, technology. Nothing in the simulation reads
//! a constant that is not reachable from here.

use serde::{Deserialize, Serialize};

use crate::civ::boats::BoatConfig;
use crate::civ::buildings::BuildConfig;
use crate::civ::economy::EconomyConfig;
use crate::civ::people::PeopleConfig;
use crate::civ::resources::{stock_map, Res, Stock, RES_COUNT};
use crate::civ::social::SocialConfig;
use crate::civ::tech::TechConfig;
use crate::civ::terrain::TerrainConfig;
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
    /// many days if nobody comes back for it.
    pub pile_life: f64,
    /// Fraction of a full load a hauler is willing to fetch for a workshop.
    pub restock_share: f64,
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
            farm_rate: 0.45,
            min_harvest_mass: 1.5,
            clear_yield: 0.5,
            pile_life: 4.0,
            restock_share: 1.0,
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
}

impl Default for StartConfig {
    fn default() -> Self {
        let mut supplies = [0.0; RES_COUNT];
        supplies[Res::Wood as usize] = 30.0;
        supplies[Res::Food as usize] = 24.0;
        supplies[Res::Fiber as usize] = 12.0;
        supplies[Res::Stone as usize] = 6.0;
        StartConfig { population: 5, supplies, storehouse: true }
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
    pub labels: bool,
    pub smoke: bool,
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
}

impl Default for ViewConfig {
    fn default() -> Self {
        ViewConfig {
            day_night: true,
            paths: true,
            deposits: true,
            people: true,
            labels: false,
            smoke: true,
            water_top: "#2b4f63".into(),
            water_deep: "#16303f".into(),
            path_color: "#6b5a44".into(),
            boats: true,
            current: true,
            detail_zoom: 1.0,
            cull: true,
        }
    }
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
            people_archive: 400,
        }
    }
}
