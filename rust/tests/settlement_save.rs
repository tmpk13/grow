//! A settlement has to survive being written down and read back: the same
//! books, the same picture, and the same next hundred days.

use grow::civ::civ_render::Detail;
use grow::civ::resources::RES_IDS;
use grow::civ::save::{capture, restore, Snapshot, SNAPSHOT_VERSION};
use grow::civ::settlement::{Rect, Settlement};
use grow::state::State;

fn run(sim: &mut Settlement, state: &State, days: f64) {
    let dt = 1.0 / state.civ.sim.tick_hz;
    let steps = (state.civ.people.day_length * days / dt).round() as i32;
    for _ in 0..steps {
        sim.step(state, dt);
        sim.plant_sim.process_raster_queue(state, 8);
    }
}

fn founded(state: &State, days: f64) -> Settlement {
    let mut sim = Settlement::new(state);
    sim.bootstrap(state);
    run(&mut sim, state, days);
    sim
}

/// Everything the panels report on, as one string, so a difference names
/// itself instead of arriving as a false assertion.
fn books(sim: &Settlement, state: &State) -> String {
    let s = sim.stats(state);
    let mut out = format!(
        "{} day {} pop {} kids {} homes {} built {} sites {} coin {:.3} \
         research {:.3} known {} born {} died {} happy {:.4} time {:.4} ticks {}",
        s.name,
        s.day,
        s.population,
        s.children,
        s.housing,
        s.buildings,
        s.sites,
        s.coin,
        s.research,
        s.known,
        s.births,
        s.deaths,
        s.happiness,
        s.time,
        s.ticks,
    );
    for colony in &s.colonies {
        out.push_str(&format!(" |{} {:.3} {:.3}", colony.name, colony.food, colony.wealth));
    }
    let stock = sim.total_stock();
    for res in RES_IDS {
        out.push_str(&format!(" {:.4}", stock[res as usize]));
    }
    for p in sim.people.live_indices() {
        let person = &sim.people[p];
        out.push_str(&format!(
            " p{}@{:.4},{:.4}/{:.4}/{:.4}",
            person.id, person.x, person.y, person.hunger, person.coin
        ));
    }
    out
}

fn picture(sim: &mut Settlement, state: &State) -> Vec<u32> {
    sim.plant_sim.process_raster_queue(state, usize::MAX);
    sim.view = Rect::whole(sim.world());
    sim.px_step = 1;
    sim.detail = Detail::Full;
    sim.composite(state);
    sim.buffer.clone()
}

#[test]
fn a_saved_settlement_comes_back_the_same() {
    let state = State::new();
    let mut original = founded(&state, 12.0);
    let json = capture(&original, &state);

    let snapshot = Snapshot::from_json(&json).expect("the file reads back");
    let mut loaded = Settlement::new(&state);
    restore(&mut loaded, &state, snapshot).expect("the world matches");

    assert_eq!(books(&loaded, &state), books(&original, &state), "the books came back different");
    assert_eq!(
        picture(&mut loaded, &state),
        picture(&mut original, &state),
        "the map came back looking different"
    );
}

/// The books above are rounded to four places to read as a report. This is the
/// same question asked to the last bit: a settlement is chaotic enough that one
/// settler's load being a fraction out is a different town a fortnight later,
/// and the way that happens is a number written exactly and parsed back badly.
#[test]
fn a_saved_settlement_comes_back_to_the_last_bit() {
    let state = State::new();
    let mut original = founded(&state, 6.0);
    let snapshot = Snapshot::from_json(&capture(&original, &state)).expect("reads back");
    let mut loaded = Settlement::new(&state);
    restore(&mut loaded, &state, snapshot).expect("the world matches");

    // Every plant comes back waiting to be drawn again, because its pixels are
    // the one thing the file leaves out. Draw both out first, or the only
    // difference found is the queue.
    original.plant_sim.process_raster_queue(&state, usize::MAX);
    loaded.plant_sim.process_raster_queue(&state, usize::MAX);

    for (what, a, b) in [
        ("the settlers", json(&original.people), json(&loaded.people)),
        ("the books", json(&original.colonies), json(&loaded.colonies)),
        ("what is standing", json(&original.buildings), json(&loaded.buildings)),
        ("what is on the ground", json(&original.piles), json(&loaded.piles)),
        ("the wilderness", json(&original.plant_sim.plants), json(&loaded.plant_sim.plants)),
    ] {
        assert_eq!(a, b, "{what} came back changed");
    }
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("writes")
}

#[test]
fn ground_drawn_on_by_hand_comes_back_with_the_settlement() {
    use grow::civ::terrain::Cell;
    use grow::world::Zone;
    let state = State::new();
    let mut original = founded(&state, 0.4);

    // A row of cells turned to water, and a row zoned bare, both of them
    // things the map would never have generated for itself here.
    let cells: Vec<(i32, i32)> = (4..12).map(|c| (c, 6)).collect();
    let zoned: Vec<(i32, i32)> = (4..12).map(|c| (c, 8)).collect();
    let painted = original.paint_cells(&cells, Cell::Water);
    original.zone_cells(&zoned, Zone::Bare);
    assert!(painted > 0, "nothing took the water");

    let snapshot = Snapshot::from_json(&capture(&original, &state)).expect("reads back");
    let mut loaded = Settlement::new(&state);
    restore(&mut loaded, &state, snapshot).expect("the world matches");

    for &(c, r) in &cells {
        assert_eq!(
            loaded.in_water(c, r),
            original.in_water(c, r),
            "the ground at {c},{r} came back different"
        );
        assert_eq!(loaded.walkable(c, r), original.walkable(c, r));
    }
    for &(c, r) in &zoned {
        assert_eq!(loaded.terrain.zone_at(c, r), Zone::Bare, "the zone at {c},{r} was lost");
    }
    assert!(!loaded.plant_sim.zones.is_empty(), "the restored wilderness was not told the zones");
}

#[test]
fn a_restored_settlement_carries_on_the_same_way() {
    let state = State::new();
    let mut original = founded(&state, 8.0);
    let snapshot = Snapshot::from_json(&capture(&original, &state)).expect("reads back");
    let mut loaded = Settlement::new(&state);
    restore(&mut loaded, &state, snapshot).expect("the world matches");

    run(&mut original, &state, 6.0);
    run(&mut loaded, &state, 6.0);

    assert_eq!(books(&loaded, &state), books(&original, &state), "the two runs came apart");
    assert_eq!(
        picture(&mut loaded, &state),
        picture(&mut original, &state),
        "the two runs drew different maps"
    );
}

/// A file is about one world. Change the map and it is no longer worth
/// anything, which is better said than half applied.
#[test]
fn a_settlement_saved_for_another_world_is_refused() {
    let state = State::new();
    let sim = founded(&state, 1.0);
    let snapshot = Snapshot::from_json(&capture(&sim, &state)).expect("reads back");

    let mut elsewhere = state.clone();
    elsewhere.civ.seed = state.civ.seed.wrapping_add(1);
    assert!(!snapshot.fits(&elsewhere));
    let mut fresh = Settlement::new(&elsewhere);
    assert!(restore(&mut fresh, &elsewhere, snapshot).is_err());
}

/// The version stamp is what stops a file written by an older shape being
/// read into this one field by field.
#[test]
fn a_file_from_another_version_is_refused() {
    let state = State::new();
    let sim = founded(&state, 1.0);
    let stamp = format!("\"version\":{SNAPSHOT_VERSION}");
    let raw = capture(&sim, &state).replace(&stamp, "\"version\":999");
    assert!(!raw.contains(&stamp), "the version stamp is not where the test looked for it");
    assert!(Snapshot::from_json(&raw).is_err());
}
