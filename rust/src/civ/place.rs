//! Putting things on the map by hand.
//!
//! The town plans for itself and the wilderness grows on its own; this is the
//! way round both, for somebody who wants a mill on that bend of the river or a
//! stand of trees where the map left none. What can be put down is what the
//! world is already made of: a building from the catalog, a plant of any
//! species the project holds, and a load of anything on the ground.
//!
//! Nothing here is a special case for the simulation. A building placed by hand
//! is the same site the planner would have laid, a plant is spawned the way the
//! wilderness spawns one, and a load is a pile like any other, so all of it is
//! picked up, built, harvested and hauled by the same rules.

use crate::civ::buildings::building_by_id;
use crate::civ::planner::can_place_at;
use crate::civ::resources::Res;
use crate::civ::settlement::Settlement;
use crate::state::State;

/// Which of the three kinds of thing the hand is holding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Building,
    Plant,
    Load,
    /// Something behind the map rather than on it: a press in the sky band
    /// puts a hill or a mountain there.
    Scenery,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Building => "Building",
            Kind::Plant => "Plant",
            Kind::Load => "Load",
            Kind::Scenery => "Scenery, behind the map",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::Building => "building",
            Kind::Plant => "plant",
            Kind::Load => "load",
            Kind::Scenery => "scenery",
        }
    }

    pub fn from_key(key: &str) -> Kind {
        match key {
            "plant" => Kind::Plant,
            "load" => Kind::Load,
            "scenery" => Kind::Scenery,
            _ => Kind::Building,
        }
    }
}

pub const KINDS: [Kind; 4] = [Kind::Building, Kind::Plant, Kind::Load, Kind::Scenery];

/// What the next press will put down. Kept as a whole rather than as one field
/// per kind so that changing what is being placed cannot leave half of the
/// last choice behind.
#[derive(Clone, Debug)]
pub struct Hand {
    pub kind: Kind,
    /// A catalog id, a species id, and how much of a load - one of each, so
    /// going back to a kind puts back what was chosen for it last.
    pub building: String,
    pub species: String,
    pub res: Res,
    pub amount: f64,
    /// A building goes up finished rather than as a site for the town to
    /// carry materials to and raise.
    pub finished: bool,
    /// The piece of scenery the next press in the sky puts up. Its `x` is
    /// wherever the press lands, so what is kept here is everything else about
    /// it: the shape, how big it is and how far off.
    pub scene: crate::civ::scenery::Scene,
}

impl Default for Hand {
    fn default() -> Self {
        Hand {
            kind: Kind::Building,
            building: "hut".to_string(),
            species: String::new(),
            res: Res::Wood,
            amount: 10.0,
            finished: false,
            scene: crate::civ::scenery::Scene::default(),
        }
    }
}

/// Puts down whatever the hand is holding, at a cell. What comes back is what
/// to say about it either way: a press that could not put anything there has to
/// say why, or the map simply looks broken.
pub fn put(
    sim: &mut Settlement,
    state: &State,
    hand: &Hand,
    col: i32,
    row: i32,
) -> Result<String, String> {
    if !sim.in_bounds(col, row) {
        return Err("off the map".to_string());
    }
    match hand.kind {
        Kind::Building => building(sim, state, hand, col, row),
        Kind::Plant => plant(sim, state, hand, col, row),
        Kind::Load => load(sim, hand, col, row),
        // Scenery is not on the map at all. It stands behind it, in the sky
        // band, and it belongs to the project rather than to the settlement,
        // so the press that puts one up never comes this way.
        Kind::Scenery => Err("scenery goes in the sky, above the land".to_string()),
    }
}

fn building(
    sim: &mut Settlement,
    state: &State,
    hand: &Hand,
    col: i32,
    row: i32,
) -> Result<String, String> {
    let def = match building_by_id(&hand.building) {
        Some(def) => def,
        None => return Err("nothing like that in the catalog".to_string()),
    };
    // The footprint is laid from the pressed cell rather than centered on it:
    // what is pressed is the corner the building is drawn from, so where it
    // lands is where it was aimed.
    if !can_place_at(sim, state, def, col, row) {
        return Err(format!("a {} will not fit there", def.label.to_lowercase()));
    }
    let ci = sim.focus.min(sim.colonies.len().saturating_sub(1));
    if sim.colonies.is_empty() {
        return Err("no town to build for".to_string());
    }
    match sim.place_building(state, ci, &hand.building, col, row, hand.finished) {
        Some(_) => {
            let town = sim.colonies[ci].name.clone();
            Ok(if hand.finished {
                format!("{} put up for {town}", def.label)
            } else {
                format!("{} laid out for {town}", def.label)
            })
        }
        None => Err("nothing was placed".to_string()),
    }
}

fn plant(
    sim: &mut Settlement,
    state: &State,
    hand: &Hand,
    col: i32,
    row: i32,
) -> Result<String, String> {
    let index = match state.species_index(&hand.species) {
        Some(i) => i,
        None => return Err("no species chosen".to_string()),
    };
    match sim.sow(state, index, col, row) {
        true => Ok(format!("{} planted", state.species[index].name)),
        false => Err("nothing will grow there".to_string()),
    }
}

fn load(sim: &mut Settlement, hand: &Hand, col: i32, row: i32) -> Result<String, String> {
    let n = hand.amount.round().max(1.0);
    match sim.add_pile(col, row, hand.res, n) {
        Some(_) => Ok(format!("{n:.0} {} left on the ground", hand.res.label())),
        None => Err("nowhere to put a load there".to_string()),
    }
}
