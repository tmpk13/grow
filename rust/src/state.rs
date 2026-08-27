//! Project state: everything the tool can save, load and export.
//!
//! The JSON shape is the one the project has always used, so a file exported
//! before this rewrite still loads. Every section carries its own defaults, and
//! serde fills a missing field from them, which is what lets an older project
//! pick up parameters that did not exist when it was saved.

use serde::{Deserialize, Serialize};

use crate::art::ArtLibrary;
use crate::civ::config::{CivConfig, SimSettings};
use crate::sampler::Materials;
use crate::shading::Shading;
use crate::species::{default_species_list, ClassLimits, Species};
use crate::world::WorldConfig;

pub const STORAGE_KEY: &str = "grow.project.v1";

/// Bumped when the world model changes shape. Version 2 turned the world from a
/// side view strip into a 2.5D area, so a version 1 world config is discarded.
/// Version 3 added the settlement: its own map, people, economy and technology.
pub const STATE_VERSION: u32 = 3;

fn default_version() -> u32 {
    STATE_VERSION
}

fn default_seed() -> u32 {
    20260815
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    pub version: u32,
    pub seed: u32,
    pub materials: Materials,
    /// Sprite sheets drawn in the tool, which settler motions can be pointed
    /// at instead of at a dropped image.
    pub art: ArtLibrary,
    pub shading: Shading,
    pub species: Vec<Species>,
    pub class_limits: ClassLimits,
    pub world: WorldConfig,
    pub sim: SimSettings,
    pub civ: CivConfig,
}

impl Default for State {
    fn default() -> Self {
        State {
            version: default_version(),
            seed: default_seed(),
            materials: Materials::new(),
            art: ArtLibrary::default(),
            shading: Shading::default(),
            species: default_species_list(),
            class_limits: ClassLimits::default(),
            world: WorldConfig::default(),
            sim: SimSettings::default(),
            civ: CivConfig::default(),
        }
    }
}

impl State {
    pub fn new() -> Self {
        State::default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn to_pretty_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Loads a project, upgrading it on the way in.
    pub fn from_json(raw: &str) -> Result<State, String> {
        let mut state: State = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        let stale = state.version < STATE_VERSION;
        if stale {
            // The world model changed shape, so an old world config is dropped
            // rather than half applied.
            state.world = WorldConfig::default();
        }
        state.version = STATE_VERSION;
        if state.species.is_empty() {
            state.species = default_species_list();
        }
        state.art.fit();
        state.materials.ensure_role_samplers();
        state.materials.invalidate();
        Ok(state)
    }

    pub fn species_index(&self, id: &str) -> Option<usize> {
        self.species.iter().position(|s| s.id == id)
    }

    pub fn find_species(&self, id: &str) -> Option<&Species> {
        self.species.iter().find(|s| s.id == id)
    }
}
