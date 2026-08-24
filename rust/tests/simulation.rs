//! The simulation has to stay reproducible: one seed, one run, one picture.
//! These are the invariants the headless smoke binaries check on a long run,
//! kept small enough to run on every `cargo test`.

use grow::civ::settlement::Settlement;
use grow::civ::terrain::Cell;
use grow::sim::Sim;
use grow::species::SIZE_CLASSES;
use grow::state::State;

fn run_lab(ticks: usize) -> (Sim, State) {
    let state = State::new();
    let mut sim = Sim::new(&state, state.world.clone());
    let dt = 1.0 / state.sim.tick_hz;
    for _ in 0..ticks {
        sim.step(&state, dt, None);
        sim.process_raster_queue(&state, 64);
    }
    sim.process_raster_queue(&state, usize::MAX);
    (sim, state)
}

#[test]
fn the_same_seed_grows_the_same_world_twice() {
    let (a, state) = run_lab(400);
    let (b, _) = run_lab(400);
    assert_eq!(a.plants.len(), b.plants.len());
    let mut sa = a;
    let mut sb = b;
    sa.composite(&state);
    sb.composite(&state);
    assert_eq!(sa.buffer, sb.buffer, "two runs of one seed differ");
}

#[test]
fn every_claimed_cell_belongs_to_a_live_plant_of_that_layer() {
    let (sim, _) = run_lab(600);
    assert!(!sim.plants.is_empty(), "nothing grew");
    for (layer, grid) in sim.world.layers.iter().enumerate() {
        for &owner in grid.iter() {
            if owner == 0 {
                continue;
            }
            let plant = sim
                .plants
                .iter()
                .find(|p| p.id == owner)
                .unwrap_or_else(|| panic!("layer {layer} claimed by missing plant {owner}"));
            assert_eq!(plant.size_class.layer(), layer, "plant is in the wrong layer");
        }
    }
}

#[test]
fn one_cell_carries_one_item_per_size_class() {
    let (sim, _) = run_lab(600);
    let mut shared = 0;
    for cy in 0..sim.world.rows {
        for cx in 0..sim.world.cols {
            let mask = sim.world.occupancy_at(cx, cy);
            if mask != 0 && mask & (mask - 1) != 0 {
                shared += 1;
            }
        }
    }
    assert!(shared > 0, "no cell holds two size classes, so the layers do nothing");
    assert!(SIZE_CLASSES.len() == 5);
}

#[test]
fn a_settlement_keeps_its_books_straight() {
    let mut state = State::new();
    // A small map with a short warmup: the point is the bookkeeping, not the
    // scale of the run.
    state.civ.world.cols = 40;
    state.civ.world.rows = 20;
    state.civ.terrain.warmup = 60.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    assert!(!sim.people.is_empty(), "nobody arrived");
    assert!(!sim.name.is_empty(), "the settlement has no name");

    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize * 3 {
        sim.step(&state, dt);
    }

    for row in 0..sim.world().rows {
        for col in 0..sim.world().cols {
            let id = sim.build_grid[sim.idx(col, row)];
            if id == 0 {
                continue;
            }
            let bi = sim.building_index(id).expect("a cell claimed by a missing building");
            let b = &sim.buildings[bi];
            assert!(col >= b.col && col < b.col + b.w && row >= b.row && row < b.row + b.h);
            assert_ne!(sim.terrain.kind[sim.terrain.idx(col, row)], Cell::Water as u8);
            assert_ne!(sim.blocked[sim.idx(col, row)], 0, "built on, but plants may still seed");
        }
    }
    for colony in &sim.colonies {
        for res in grow::civ::resources::RES_IDS {
            let have = colony.stock[res as usize];
            assert!(have >= -0.001, "{} has negative {}", colony.name, res.id());
            assert!(
                colony.stock_reserved[res as usize] <= have + 0.001,
                "{} has more {} reserved than in store",
                colony.name,
                res.id()
            );
        }
    }
    for p in sim.people.iter() {
        assert!(p.x >= 0.0 && p.x <= sim.world().cols as f64, "{} walked off the map", p.name);
        assert!(p.y >= 0.0 && p.y <= sim.world().rows as f64, "{} walked off the map", p.name);
        if p.carry.n > 0.0 {
            assert!(p.carry.res.is_some(), "{} carries nothing in particular", p.name);
        }
    }
    for plant in &sim.plant_sim.plants {
        assert_eq!(
            sim.blocked[sim.idx(plant.col, plant.row)], 0,
            "a plant grows on blocked ground"
        );
    }
}

/// The two halves of a deed have to agree, or a house ends up owned by
/// somebody who does not think they own it and can never be upgraded.
#[test]
fn every_deed_has_two_ends() {
    let mut state = State::new();
    state.civ.world.cols = 48;
    state.civ.world.rows = 24;
    state.civ.terrain.warmup = 60.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize * 6 {
        sim.step(&state, dt);
    }
    for b in &sim.buildings {
        if b.owner == 0 {
            continue;
        }
        let owner = sim.people.get(b.owner).expect("a deed held by nobody on file");
        assert!(owner.alive, "a deed held by somebody dead");
        assert_eq!(owner.owns, b.id, "{} does not agree they own it", owner.name);
    }
    for p in sim.people.iter() {
        if p.owns == 0 {
            continue;
        }
        let bi = sim.building_index(p.owns).expect("a deed to a building that is gone");
        assert_eq!(sim.buildings[bi].owner, p.id, "the house disagrees with {}", p.name);
    }
}

/// A town has to outlive the people who founded it.
///
/// Two separate bugs both showed up only here: births credited to the same
/// couple every day (so the next generation were all siblings and none of them
/// could marry), and a couple's fertility tested on one arbitrary partner (so a
/// household whose house was being rebuilt stopped having children). Both leave
/// a town that grows, stalls, and dies out with a full storehouse, which no
/// bookkeeping check notices.
#[test]
fn a_town_outlives_its_founders() {
    let mut state = State::new();
    state.civ.world.cols = 56;
    state.civ.world.rows = 28;
    state.civ.terrain.warmup = 120.0;
    // Shorter days and faster aging, so two generations fit in a test. Work
    // rates are per second and aging is per day, so pushing this much further
    // starves the town for reasons that have nothing to do with demographics.
    state.civ.people.day_length = 60.0;
    state.civ.people.years_per_day = 1.0;
    state.civ.start.supplies[grow::civ::resources::Res::Food as usize] = 60.0;
    state.civ.start.supplies[grow::civ::resources::Res::Wood as usize] = 60.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let founders = sim.people.count();
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize * 100 {
        sim.step(&state, dt);
    }

    assert!(sim.people.count() > 0, "the town died out");
    assert!(
        sim.births as usize > founders,
        "only {} children in a hundred days from {founders} founders",
        sim.births
    );
    // Births have to be spread over the couples. One mother accounting for most
    // of a generation is the failure, and it is not enough to check that more
    // than one mother exists: the one couple picked every day changes as people
    // age out, so a broken run still ends up with a handful of them.
    let mut per_mother: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for p in sim.people.archive().iter().filter(|p| p.mother != 0) {
        *per_mother.entry(p.mother).or_default() += 1;
    }
    let children: usize = per_mother.values().sum();
    let biggest = per_mother.values().copied().max().unwrap_or(0);
    assert!(
        biggest * 2 <= children,
        "one mother had {biggest} of the town's {children} children: births are not \
         being spread over its couples"
    );
    let married = sim.people.iter().filter(|p| p.spouse != 0).count();
    let adults = sim.people.iter().filter(|p| p.adult()).count();
    assert!(
        married >= 4,
        "only {married} of {adults} adults are married, in a town of {}",
        sim.people.count()
    );
}

/// Everyone the register hands out has to be alive, and the dead have to stay
/// dead: a burial that runs twice inflates the death count and re-inherits the
/// house every tick.
#[test]
fn the_dead_are_buried_once() {
    let mut state = State::new();
    state.civ.world.cols = 40;
    state.civ.world.rows = 20;
    state.civ.terrain.warmup = 40.0;
    // Short lives, so somebody actually dies inside the run.
    state.civ.people.years_per_day = 6.0;
    state.civ.people.lifespan_min = 30;
    state.civ.people.lifespan_max = 40;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize * 12 {
        sim.step(&state, dt);
    }
    assert!(sim.deaths > 0, "nobody died, so this proves nothing");
    assert_eq!(
        sim.deaths as usize,
        sim.people.buried() as usize,
        "the death count and the register disagree"
    );
    for p in sim.people.iter() {
        assert!(p.alive, "the register handed out a dead settler");
    }
}

/// A town that walls itself has to leave itself a way out.
///
/// The ring is the one thing in the settlement that can make the map
/// impassable, and it does it quietly: every piece is legal on its own, and
/// nothing about a sealed town is inconsistent. What is checked here is the
/// rule that stops it - a piece is only ever queued if the ground outside it
/// can still reach the middle of the town with that piece shut - and the two
/// facts the pathfinder depends on, that a finished gate is walkable and a
/// finished wall is not.
#[test]
fn a_ring_never_shuts_the_town_in() {
    let mut state = State::new();
    state.civ.world.cols = 48;
    state.civ.world.rows = 24;
    state.civ.terrain.warmup = 60.0;
    // Wall from the first day and cheaply, so a ring actually goes up inside a
    // test rather than two hundred simulated days later.
    state.civ.build.wall_population = 1;
    state.civ.build.wall_sites = 3;
    state.civ.build.wall_margin = 2;
    state.civ.build.cost_scale = 0.2;
    state.civ.build.work_scale = 0.2;
    state.civ.start.supplies[grow::civ::resources::Res::Wood as usize] = 300.0;
    state.civ.start.supplies[grow::civ::resources::Res::Plank as usize] = 120.0;
    state.civ.start.supplies[grow::civ::resources::Res::Stone as usize] = 120.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    for ci in 0..sim.colonies.len() {
        sim.colonies[ci].tech.known = vec!["stonework", "carpentry", "fortification"];
        sim.colonies[ci].refresh_tech();
    }

    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize * 40 {
        sim.step(&state, dt);
    }

    let ring: Vec<usize> = (0..sim.buildings.len())
        .filter(|&bi| sim.buildings[bi].built && sim.buildings[bi].def.structure.perimeter())
        .collect();
    assert!(!ring.is_empty(), "the town never raised a single piece of wall");

    for &bi in &ring {
        let b = &sim.buildings[bi];
        let through = b.def.structure.passable();
        for row in b.row..b.row + b.h {
            for col in b.col..b.col + b.w {
                assert_eq!(
                    sim.walkable(col, row),
                    through,
                    "{} at {col},{row} is walkable: {}, but it should be {through}",
                    b.def.id,
                    sim.walkable(col, row)
                );
            }
        }
    }

    for pi in sim.people.live_indices() {
        if sim.people[pi].aboard != 0 {
            continue;
        }
        let (c, r) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
        let ci = sim.colony_of(pi);
        let center = sim.colonies[ci].center;
        let name = sim.people[pi].name.clone();
        assert!(
            sim.find_path(c, r, center.0, center.1).is_some(),
            "{name} at {c},{r} is walled out of their own town"
        );
    }
}

/// Everyone a settler has met keeps a slot, and the register of them stays
/// sane: nobody is their own acquaintance, nobody is filed twice, and the
/// memory cap is a cap on the people somebody merely met rather than on their
/// family.
#[test]
fn settlers_remember_the_people_they_meet() {
    let mut state = State::new();
    state.civ.world.cols = 44;
    state.civ.world.rows = 22;
    state.civ.terrain.warmup = 60.0;
    state.civ.social.memory = 10;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize * 20 {
        sim.step(&state, dt);
    }

    let known: usize = sim.people.iter().map(|p| p.bonds.len()).sum();
    assert!(known > 0, "twenty days in one town and nobody has met anybody");

    for p in sim.people.archive() {
        let mut seen: Vec<u32> = Vec::new();
        for bond in &p.bonds {
            assert_ne!(bond.who, p.id, "{} keeps a bond with themself", p.name);
            assert!(!seen.contains(&bond.who), "{} is filed twice by {}", bond.who, p.name);
            seen.push(bond.who);
            assert!(
                bond.affinity.is_finite() && bond.affinity.abs() <= 1.001,
                "{} feels {} about somebody",
                p.name,
                bond.affinity
            );
        }
        assert!(
            p.met_count() <= state.civ.social.memory,
            "{} remembers {} people they merely met, and the cap is {}",
            p.name,
            p.met_count(),
            state.civ.social.memory
        );
    }

    // A marriage is a bond in its own right, on both sides, whether or not the
    // two of them have happened to stand next to each other since.
    for p in sim.people.iter() {
        if p.spouse == 0 {
            continue;
        }
        let bond = p.bond_with(p.spouse);
        assert!(bond.is_some_and(|b| b.kin), "{} does not have their spouse on file", p.name);
    }
}

/// A stall is the one thing that moves coin from one settler to another with
/// nothing in between: the buyer's purse pays the keeper's, and the town's
/// treasury never sees it.
#[test]
fn a_stall_moves_coin_from_one_settler_to_another() {
    use grow::civ::resources::Res;

    let mut state = State::new();
    state.civ.world.cols = 40;
    state.civ.world.rows = 20;
    state.civ.terrain.warmup = 40.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);

    let center = sim.colonies[0].center;
    let def = grow::civ::buildings::building_by_id("stall").expect("no stall in the catalog");
    let site = grow::civ::planner::find_site_near(&sim, &state, 0, def, center.0, center.1, 8)
        .expect("nowhere to put a stall");
    let bi = sim
        .place_building(&state, 0, "stall", site.0, site.1, true)
        .expect("the stall would not stand");

    let live = sim.people.live_indices();
    let (keeper, shopper) = (live[0], live[1]);
    let keeper_id = sim.people[keeper].id;
    sim.buildings[bi].owner = keeper_id;
    sim.people[keeper].stall = sim.buildings[bi].id;
    sim.people[keeper].coin = 0.0;
    // Something on the counter, and somebody with the coin and the appetite for
    // it standing next to it.
    sim.buildings[bi].inv[Res::Food as usize] = 30.0;
    sim.people[shopper].coin = 120.0;
    sim.people[shopper].x = site.0 as f64 + 0.5;
    sim.people[shopper].y = site.1 as f64 + 1.5;
    let purse = sim.people[shopper].coin;

    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize * 6 {
        sim.step(&state, dt);
    }

    let sold = 30.0 - sim.buildings[bi].inv[Res::Food as usize];
    assert!(sold > 0.0, "nothing was ever sold over the counter");
    assert!(
        sim.people[keeper].coin > 0.0,
        "{} sold {sold} food and has no coin to show for it",
        sim.people[keeper].name
    );
    assert!(
        sim.people[shopper].coin < purse,
        "the buyer's purse never moved"
    );
}
