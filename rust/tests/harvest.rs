//! Cutting by hand: what a hold on the map does to the world, what a hold that
//! was not long enough does not do, and what the towns make of both.

use grow::civ::harvest::{Cut, Lore};
use grow::civ::resources::Res;
use grow::civ::save::{capture, restore, Snapshot};
use grow::civ::settlement::Settlement;
use grow::civ::tasks::{choose_task, Task};
use grow::state::State;

/// A small map, warmed up enough that there is something growing on it.
fn founded() -> (Settlement, State) {
    let mut state = State::new();
    state.civ.world.cols = 48;
    state.civ.world.rows = 24;
    state.civ.terrain.warmup = 90.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    (sim, state)
}

/// The middle of the cell a plant is rooted in, which is a point every plant is
/// pointed at by whatever shape it has grown into.
fn foot_of(sim: &Settlement, id: i32) -> (f64, f64) {
    let i = sim.plant_sim.plant_index(id).expect("the plant is on the map");
    let plant = &sim.plant_sim.plants[i];
    (plant.col as f64 + 0.5, plant.row as f64 + 0.5)
}

/// The biggest thing growing, which is the one a hold is surest to be aimed at.
fn biggest(sim: &Settlement) -> i32 {
    let mut best = (0.0, 0);
    for plant in &sim.plant_sim.plants {
        let mass = sim.plant_mass(plant);
        if mass > best.0 {
            best = (mass, plant.id);
        }
    }
    assert!(best.1 != 0, "nothing grew to cut");
    best.1
}

/// Holds the pointer on a point for a while, a frame at a time, and says how
/// many plants came down while it was there.
fn hold(sim: &mut Settlement, state: &State, at: Option<(f64, f64)>, seconds: f64) -> usize {
    let dt = 1.0 / 60.0;
    let mut cuts = 0;
    for _ in 0..(seconds / dt).round() as usize {
        if sim.hand_harvest(state, at, dt).is_some() {
            cuts += 1;
        }
    }
    cuts
}

/// Holds until the first thing under the pointer has been cut, and says what
/// it yielded. A hold that is not let go of goes on cutting whatever is left
/// where it is pointing, which is what a drag across a patch is made of.
fn cut_once(sim: &mut Settlement, state: &State, at: (f64, f64)) -> Cut {
    let dt = 1.0 / 60.0;
    for _ in 0..600 {
        if let Some(cut) = sim.hand_harvest(state, Some(at), dt) {
            return cut;
        }
    }
    panic!("ten seconds of holding cut nothing");
}

/// A point to press on and the plant that press would take. Aimed at the foot
/// of the biggest thing growing, then asked of the map which plant is actually
/// under it: a crown hangs over its neighbors and the answer is not always the
/// plant the point was worked out from.
fn aim(sim: &Settlement, state: &State) -> ((f64, f64), i32) {
    let at = foot_of(sim, biggest(sim));
    let hit = sim.harvestable_at(state, at.0, at.1).expect("nothing is under the pointer");
    (at, hit)
}

#[test]
fn a_cut_by_hand_leaves_what_it_was_worth_on_the_ground() {
    let (mut sim, state) = founded();
    let (at, id) = aim(&sim, &state);
    let before = sim.piles.len();
    let was = sim.plant_mass(&sim.plant_sim.plants[sim.plant_sim.plant_index(id).unwrap()]);

    let cut = cut_once(&mut sim, &state, at);
    assert!(!cut.gains.is_empty(), "the cut paid out nothing at all");
    let left = sim
        .plant_sim
        .plant_index(id)
        .map(|i| sim.plant_mass(&sim.plant_sim.plants[i]))
        .unwrap_or(0.0);
    assert!(left < was, "the plant under the pointer is the size it was");
    assert!(sim.piles.len() > before, "the cut left nothing on the ground");
    assert!(
        sim.piles.iter().any(|p| p.by_hand && p.n > 0.0),
        "what was cut is not marked as asked for, so nobody will hurry for it",
    );
    // Nothing is carried: the hand has nowhere to put anything.
    assert!(sim.people.iter().all(|p| p.carry.n < 0.001 || p.carry.res.is_some()));
    assert!(sim.hand.is_empty(), "a finished cut is still on the pointer");
}

#[test]
fn a_hold_that_was_not_long_enough_leaves_no_mark() {
    let (mut sim, state) = founded();
    let (at, _) = aim(&sim, &state);
    let plants = sim.plant_sim.plants.len();
    let piles = sim.piles.len();

    // Long enough for a bar to come up, nowhere near long enough to finish:
    // the biggest plant on the map is seconds of holding.
    let cuts = hold(&mut sim, &state, Some(at), 0.2);
    assert_eq!(cuts, 0, "a moment's press took a plant down");
    assert!(!sim.hand.is_empty(), "nothing is showing as part cut");

    // Let go, and after the delay the bar and the progress in it are gone.
    hold(&mut sim, &state, None, 2.0);
    assert!(sim.hand.is_empty(), "a cut that was given up on is still on the pointer");
    assert_eq!(sim.plant_sim.plants.len(), plants, "the plant came down anyway");
    assert_eq!(sim.piles.len(), piles, "a cut that never finished still paid out");
}

#[test]
fn ground_cover_is_cut_back_rather_than_pulled_up() {
    let (mut sim, state) = founded();
    // The mat has to be the thing the press would actually take, not merely a
    // mat that exists: something taller standing in the same cell would be
    // what came down.
    let mats: Vec<(f64, f64, i32)> = sim
        .plant_sim
        .plants
        .iter()
        .filter(|p| p.size_class == grow::species::SizeClass::Ground)
        .filter(|p| sim.plant_mass(p) >= grow::civ::harvest::min_mass(&state) * 2.0)
        .map(|p| (p.col as f64 + 0.5, p.row as f64 + 0.5, p.id))
        .collect();
    let target = mats
        .iter()
        .find(|&&(x, y, id)| sim.harvestable_at(&state, x, y) == Some(id))
        .copied();
    let (x, y, id) = match target {
        Some(t) => t,
        // A map with no ground cover clear of anything taller has nothing to
        // say about cutting ground cover back.
        None => return,
    };
    let was = sim.plant_mass(&sim.plant_sim.plants[sim.plant_sim.plant_index(id).unwrap()]);

    let cut = cut_once(&mut sim, &state, (x, y));
    assert!(cut.cut_back, "a mat was pulled up rather than cut back");
    let index = sim.plant_sim.plant_index(id).expect("a cut back mat was taken away");
    let now = sim.plant_mass(&sim.plant_sim.plants[index]);
    assert!(now < was, "the mat was cut back to the same size it was");
    assert!(now > 0.0, "there is nothing left of a mat that is meant to grow again");
}

#[test]
fn what_was_cut_by_hand_is_fetched_before_a_settler_finds_their_own_work() {
    let (mut sim, state) = founded();
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..400 {
        sim.step(&state, dt);
    }
    let pi = sim.people.live_indices().into_iter().find(|&i| sim.people[i].adult()).expect("nobody");
    // Empty handed and standing still, so the decision is between fetching
    // this and going off to do whatever they usually do.
    sim.people[pi].carry.n = 0.0;
    sim.people[pi].carry.res = None;
    let (col, row) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    sim.add_hand_pile(col, row, Res::Wood, 6.0);
    let pile = sim.piles.iter().find(|p| p.by_hand).map(|p| p.id).expect("no pile was left");

    choose_task(&mut sim, &state, pi);
    match &sim.people[pi].task {
        Some(Task::Pickup { pile_id }) => assert_eq!(*pile_id, pile),
        other => panic!("a load that was asked for was passed over for {other:?}"),
    }
}

#[test]
fn a_species_that_is_cut_for_is_one_the_gatherers_learn_to_want() {
    let mut lore = Lore::default();
    assert_eq!(lore.interest("oak"), 0.0);
    lore.teach("oak", 4.0);
    let once = lore.interest("oak");
    lore.teach("oak", 40.0);
    let often = lore.interest("oak");
    assert!(once > 0.0 && often > once, "cutting more of a species taught less of it");
    assert!(often < 1.0, "the lesson has no end to it");
    assert!(lore.interest("pine") == 0.0, "cutting an oak taught them about pines");

    // The gatherers read this off the plant buckets, which is the only place
    // any of them look.
    let (mut sim, state) = founded();
    let (at, id) = aim(&sim, &state);
    let species =
        sim.plant_sim.plants[sim.plant_sim.plant_index(id).unwrap()].species_id.clone();
    cut_once(&mut sim, &state, at);
    assert!(!sim.lore.is_empty(), "the cut taught nothing");
    let mut taught = 0;
    for bucket in &sim.plant_index.buckets {
        for mark in bucket {
            let plant = &sim.plant_sim.plants[sim.plant_sim.plant_index(mark.id).unwrap()];
            if plant.species_id == species {
                assert!(mark.lore > 0.0, "a mark of a taught species carries no lore");
                taught += 1;
            } else {
                assert_eq!(mark.lore, 0.0, "lore leaked onto another species");
            }
        }
    }
    assert!(taught > 0, "nothing of the cut species is left to be wanted");
}

#[test]
fn what_the_hand_taught_survives_a_save() {
    let (mut sim, state) = founded();
    let (at, _) = aim(&sim, &state);
    cut_once(&mut sim, &state, at);
    let before = sim.lore.known().iter().map(|&(id, n)| (id.to_string(), n)).collect::<Vec<_>>();
    let hand_piles = sim.piles.iter().filter(|p| p.by_hand).count();
    assert!(!before.is_empty() && hand_piles > 0);

    let raw = capture(&sim, &state);
    let snap = Snapshot::from_json(&raw).expect("the file reads back");
    let mut back = Settlement::new(&state);
    restore(&mut back, &state, snap).expect("the file fits the world it was made on");

    let after = back.lore.known().iter().map(|&(id, n)| (id.to_string(), n)).collect::<Vec<_>>();
    assert_eq!(before, after, "the lesson was lost in the save");
    assert_eq!(
        back.piles.iter().filter(|p| p.by_hand).count(),
        hand_piles,
        "loads that were asked for came back as ordinary ones",
    );
}
