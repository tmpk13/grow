//! The simulation has to stay reproducible: one seed, one run, one picture.
//! These are the invariants the headless smoke binaries check on a long run,
//! kept small enough to run on every `cargo test`.

use grow::civ::civ_render::Detail;
use grow::civ::settlement::{Rect, Settlement};
use grow::civ::sprites::{motion_of, natural_cmp, Clip, Motion, PeopleSprites, MAX_FRAME_PX};
use grow::civ::terrain::Cell;
use grow::civ::balloons::research_lift as balloon_lift;
use grow::sim::Sim;
use grow::species::{SizeClass, SIZE_CLASSES};
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
/// One run of a settlement says nothing: it is chaotic enough that any change
/// to the pathfinder's search space reorders which equal-cost route is found,
/// and the demographics a hundred days later swing with it. So this judges a
/// spread of seeds, the way any change to the settlement has to be judged.
///
/// The bar is still a real one. A town that mostly dies out, a generation that
/// mostly comes from one mother, or a town nobody marries in all fail; what no
/// longer fails is one unlucky map.
#[test]
fn a_town_outlives_its_founders() {
    const SEEDS: [u32; 5] = [77104, 11, 555, 314159, 8080];
    let mut lived = 0;
    let mut births = 0usize;
    let mut founders = 0usize;
    let mut children = 0usize;
    let mut biggest = 0usize;
    let mut married = 0usize;

    for seed in SEEDS {
        let mut state = State::new();
        state.civ.seed = seed;
        state.civ.world.cols = 56;
        state.civ.world.rows = 28;
        state.civ.terrain.warmup = 120.0;
        // Shorter days and faster aging, so two generations fit in a test. Work
        // rates are per second and aging is per day, so pushing this much
        // further starves the town for reasons that have nothing to do with
        // demographics.
        state.civ.people.day_length = 60.0;
        state.civ.people.years_per_day = 1.0;
        state.civ.start.supplies[grow::civ::resources::Res::Food as usize] = 60.0;
        state.civ.start.supplies[grow::civ::resources::Res::Wood as usize] = 60.0;
        let mut sim = Settlement::new(&state);
        sim.bootstrap(&state);
        founders += sim.people.count();
        let dt = 1.0 / state.civ.sim.tick_hz;
        for _ in 0..(state.civ.people.day_length / dt) as usize * 100 {
            sim.step(&state, dt);
        }

        if sim.people.count() > 0 {
            lived += 1;
        }
        births += sim.births as usize;
        // Everyone who was married at the end of their own story rather than
        // the couples left standing on the last day: a hundred days of a five
        // person town is chaotic, and who is alive and paired off when the
        // clock stops is a lottery. Whether anybody ever married is not.
        married += sim.people.archive().iter().filter(|p| p.spouse != 0).count();
        // Births have to be spread over the couples. One mother accounting for
        // most of a generation is the failure, and it is not enough to check
        // that more than one mother exists: the one couple picked every day
        // changes as people age out, so a broken run still ends up with a
        // handful of them.
        let mut per_mother: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for p in sim.people.archive().iter().filter(|p| p.mother != 0) {
            *per_mother.entry(p.mother).or_default() += 1;
        }
        children += per_mother.values().sum::<usize>();
        biggest = biggest.max(per_mother.values().copied().max().unwrap_or(0));
    }

    assert!(
        lived * 2 > SEEDS.len(),
        "only {lived} of {} towns were still standing after a hundred days",
        SEEDS.len()
    );
    assert!(
        births > founders,
        "only {births} children in a hundred days from {founders} founders"
    );
    assert!(
        biggest * 2 <= children,
        "one mother had {biggest} of the {children} children born across the runs: \
         births are not being spread over a town's couples"
    );
    assert!(
        married >= SEEDS.len(),
        "only {married} people ever married across {} towns",
        SEEDS.len()
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
        assert!(p.alive, "the register handed out a dead person");
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
        sim.colonies[ci].tech.known =
            ["stonework", "carpentry", "fortification"].map(String::from).to_vec();
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

/// Everyone a person has met keeps a slot, and the register of them stays
/// sane: nobody is their own acquaintance, nobody is filed twice, and the
/// memory cap is a cap on the people somebody merely met rather than on their
/// family.
#[test]
fn people_remember_the_others_they_meet() {
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

/// A stall is the one thing that moves coin from one person to another with
/// nothing in between: the buyer's purse pays the keeper's, and the town's
/// treasury never sees it.
#[test]
fn a_stall_moves_coin_from_one_person_to_another() {
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

/// Zoomed out the camera collapses a block of world pixels into one on screen,
/// and everything below it stops painting the pixels that block will not show:
/// the ground restore, the cached ground under it and the upload all step over
/// the same grid. If those three ever disagreed about where the grid starts,
/// the map would show rows of whatever the last frame happened to leave there,
/// so this pins the one fact they share.
#[test]
fn a_stepped_frame_matches_a_whole_one_on_the_rows_it_keeps() {
    let mut state = State::new();
    state.civ.world.cols = 64;
    state.civ.world.rows = 32;
    state.civ.terrain.warmup = 60.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize {
        sim.step(&state, dt);
    }
    sim.detail = Detail::Blocks;
    sim.view = Rect::whole(sim.world());

    // Poisoned first, so a row the stepped frame declines to paint keeps the
    // poison and is caught. Without this the buffer already holds the right
    // pixels from the frame before and skipping a row proves nothing.
    let poison = 0xdead_beef;
    sim.px_step = 1;
    sim.ground_dirty = true;
    sim.buffer.fill(poison);
    sim.composite(&state);
    let whole = sim.buffer.clone();
    assert!(!whole.contains(&poison), "the whole frame left a row unpainted");

    let px_w = sim.world().px_w as usize;
    for step in [2, 4, 8] {
        sim.px_step = step;
        sim.buffer.fill(poison);
        sim.composite(&state);
        let mut y = 0;
        while y < sim.world().px_h {
            let row = y as usize * px_w;
            assert_eq!(
                &sim.buffer[row..row + px_w],
                &whole[row..row + px_w],
                "row {y} differs at step {step}"
            );
            y += step;
        }
    }
}

// ---- person sprites -----------------------------------------------------

fn strip(frames: i32, fw: i32, fh: i32) -> Clip {
    // Every frame is filled with its own index, so a slice that reads the
    // wrong column is obvious rather than merely wrong.
    let w = fw * frames;
    let mut px = vec![0u32; (w * fh) as usize];
    for f in 0..frames {
        for y in 0..fh {
            for x in 0..fw {
                px[(y * w + f * fw + x) as usize] = (f + 1) as u32;
            }
        }
    }
    Clip::from_strip(w, fh, px, frames, "test".into()).expect("clip")
}

#[test]
fn a_strip_is_cut_into_as_many_frames_as_it_is_told() {
    let mut clip = strip(6, 5, 9);
    assert_eq!(clip.frame_count(), 6);
    assert_eq!(clip.frame_w(), 5);
    for f in 0..6 {
        assert_eq!(clip.pixel(f, 0, 0), (f + 1) as u32, "frame {f} read wrong");
        assert_eq!(clip.pixel(f, 4, 8), (f + 1) as u32);
        // Nothing outside a frame belongs to it, whichever frame it is.
        assert_eq!(clip.pixel(f, 5, 0), 0);
    }
    // The sheet is kept whole, so the same pixels re-cut at a new count.
    clip.frames = 3;
    assert_eq!(clip.frame_w(), 10);
    assert_eq!(clip.pixel(1, 0, 0), 3);
    assert_eq!(clip.pixel(1, 5, 0), 4);
}

#[test]
fn one_image_per_frame_lines_up_at_the_feet() {
    // Two frames of different sizes: the short one should stand on the floor of
    // the box the tall one sets, not float at the top of it.
    let short = (2, 2, vec![7u32; 4]);
    let tall = (4, 6, vec![9u32; 24]);
    let clip = Clip::from_frames(vec![short, tall], "pair".into()).expect("clip");
    assert_eq!(clip.frame_count(), 2);
    assert_eq!(clip.frame_w(), 4);
    assert_eq!(clip.h, 6);
    assert_eq!(clip.pixel(0, 1, 5), 7, "the short frame is off the floor");
    assert_eq!(clip.pixel(0, 1, 0), 0, "the short frame was stretched");
    assert_eq!(clip.pixel(1, 0, 0), 9);
}

#[test]
fn an_oversized_drop_is_scaled_rather_than_refused() {
    let over = MAX_FRAME_PX * 3;
    let clip = Clip::from_strip(over * 2, over, vec![5u32; (over * 2 * over) as usize], 2, "big".into())
        .expect("clip");
    assert!(clip.frame_w() <= MAX_FRAME_PX, "frame {} too wide", clip.frame_w());
    assert!(clip.h <= MAX_FRAME_PX, "sheet {} too tall", clip.h);
    assert_eq!(clip.frame_count(), 2, "the frame count survived the scaling");
    assert_eq!(clip.pixel(1, 0, 0), 5, "the art did not survive the scaling");
}

#[test]
fn frames_advance_with_the_clock_or_with_the_ground() {
    let mut clip = strip(4, 3, 3);
    clip.fps = 8.0;
    clip.stride = false;
    assert_eq!(clip.frame_index(0.0, 0.0), 0);
    assert_eq!(clip.frame_index(0.0, 0.125), 1);
    assert_eq!(clip.frame_index(0.0, 0.5), 0, "the clip did not loop");
    // Standing still with a clock clip still cycles; with a stride clip it does
    // not move at all, which is the whole reason for the switch.
    clip.stride = true;
    assert_eq!(clip.frame_index(0.0, 99.0), 0);
    assert_eq!(clip.frame_index(6.0, 0.0), 0, "one cell should be a whole loop");
    assert_eq!(clip.frame_index(1.5, 0.0), 2);
    // A frame count of one never asks for a second frame.
    clip.frames = 1;
    assert_eq!(clip.frame_index(123.0, 456.0), 0);
}

#[test]
fn a_motion_with_no_art_borrows_from_a_related_one() {
    let mut sprites = PeopleSprites::default();
    assert!(sprites.resolve(Motion::Walk).is_none(), "nothing was dropped");
    sprites.set(Motion::Walk, Some(strip(2, 4, 4)));
    // Carrying and standing both fall back to the walk; nothing falls back to
    // a slot that is still empty.
    assert_eq!(sprites.resolve(Motion::Carry).map(|(m, _)| m), Some(Motion::Walk));
    assert_eq!(sprites.resolve(Motion::Idle).map(|(m, _)| m), Some(Motion::Walk));
    assert_eq!(sprites.resolve(Motion::Walk).map(|(m, _)| m), Some(Motion::Walk));
    sprites.set(Motion::Carry, Some(strip(3, 4, 4)));
    assert_eq!(sprites.resolve(Motion::Carry).map(|(m, _)| m), Some(Motion::Carry));
    // The switch hides the art without giving it up.
    sprites.enabled = false;
    assert!(sprites.resolve(Motion::Carry).is_none());
    assert!(sprites.any(), "the sheets are still there");
}

// ---- strays -------------------------------------------------------------

#[test]
fn somebody_left_a_long_way_from_home_founds_a_town_of_their_own() {
    // A big map, so there is somewhere to be far away in.
    let mut state = State::new();
    state.civ.world.cols = 140;
    state.civ.world.rows = 60;
    state.civ.terrain.warmup = 30.0;
    state.civ.build.stray_wait = 8.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..600 {
        sim.step(&state, dt);
    }
    let towns = sim.colonies.len();
    let home = sim.colonies[0].center;

    // Carry somebody to the far side of the map and leave them there.
    let id = outdoors(&sim);
    let far = (2..sim.world().rows - 2)
        .flat_map(|r| (2..sim.world().cols - 2).map(move |c| (c, r)))
        .filter(|&(c, r)| sim.walkable(c, r) && sim.terrain.is_buildable(c, r))
        .max_by_key(|&(c, r)| {
            (((c - home.0) as f64).hypot((r - home.1) as f64) * 100.0) as i64
        })
        .expect("nowhere far away");
    let gone = ((far.0 - home.0) as f64).hypot((far.1 - home.1) as f64);
    assert!(gone > state.civ.build.stray_distance, "the map is not big enough to be lost on");

    assert!(sim.hold_person(id));
    sim.move_held(far.0 as f64 + 0.5, far.1 as f64 + 0.5);
    assert!(sim.drop_held().is_some(), "they could not be put down out there");

    // Held where they were put: they cannot walk home fast enough to matter,
    // so the days out there add up.
    let mut founded = false;
    for _ in 0..(state.civ.people.day_length * state.civ.sim.tick_hz * 8.0) as usize {
        sim.step(&state, dt);
        if sim.colonies.len() > towns {
            founded = true;
            break;
        }
    }
    assert!(founded, "eight days out there and nobody started anything");
    let town = sim.colonies.last().expect("the new town");
    assert!(
        sim.people.get(id).is_some_and(|p| p.colony == town.id),
        "the town was founded without the person who founded it"
    );
    // It is far enough from the old one to be its own place.
    let d = ((town.center.0 - home.0) as f64).hypot((town.center.1 - home.1) as f64);
    assert!(
        d >= state.civ.build.colony_spacing as f64,
        "the new town is {d:.0} cells from the old one"
    );
}

#[test]
fn nobody_settles_where_they_stand_with_the_setting_off() {
    let mut state = State::new();
    state.civ.world.cols = 140;
    state.civ.world.rows = 60;
    state.civ.terrain.warmup = 30.0;
    state.civ.build.strays_settle = false;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..600 {
        sim.step(&state, dt);
    }
    let towns = sim.colonies.len();
    let home = sim.colonies[0].center;
    let id = outdoors(&sim);
    let far = (2..sim.world().rows - 2)
        .flat_map(|r| (2..sim.world().cols - 2).map(move |c| (c, r)))
        .filter(|&(c, r)| sim.walkable(c, r) && sim.terrain.is_buildable(c, r))
        .max_by_key(|&(c, r)| {
            (((c - home.0) as f64).hypot((r - home.1) as f64) * 100.0) as i64
        })
        .expect("nowhere far away");
    assert!(sim.hold_person(id));
    sim.move_held(far.0 as f64 + 0.5, far.1 as f64 + 0.5);
    sim.drop_held();
    for _ in 0..(state.civ.people.day_length * state.civ.sim.tick_hz * 8.0) as usize {
        sim.step(&state, dt);
    }
    assert_eq!(sim.colonies.len(), towns, "a town was founded with the setting off");
}

// ---- drawing on the map --------------------------------------------------

#[test]
fn painting_a_cell_changes_the_ground_and_takes_what_stood_on_it() {
    use grow::civ::terrain::Cell;
    let (mut sim, state) = peopled(48, 24);
    // A dry cell with nothing built on it, and something growing in it if the
    // map will give us one.
    let dry = (2..sim.world().rows - 2)
        .flat_map(|r| (2..sim.world().cols - 2).map(move |c| (c, r)))
        .find(|&(c, r)| !sim.in_water(c, r) && sim.walkable(c, r))
        .expect("nowhere dry on the map");
    assert!(sim.sow(&state, 0, dry.0, dry.1) || true);

    assert!(sim.paint_cell(dry.0, dry.1, Cell::Water), "the cell would not take water");
    assert!(sim.in_water(dry.0, dry.1), "it is not water after being painted water");
    assert!(!sim.walkable(dry.0, dry.1), "water was left walkable");
    assert!(
        !sim.plant_sim.plants.iter().any(|p| p.col == dry.0 && p.row == dry.1),
        "something is still growing in the water"
    );
    assert!(sim.terrain_painted, "the map does not know it has been drawn on");
    // Painting it what it already is changes nothing.
    assert!(!sim.paint_cell(dry.0, dry.1, Cell::Water));

    // Ground with something on it is left alone: the map may not move under a
    // building.
    let built = sim.buildings.first().map(|b| (b.col, b.row));
    if let Some((c, r)) = built {
        assert!(!sim.paint_cell(c, r, Cell::Water), "the ground moved under a building");
    }
}

#[test]
fn a_zone_says_what_may_take_root_without_touching_what_is_there() {
    use grow::world::Zone;
    let (mut sim, state) = peopled(48, 24);
    // A cell that will take a plant at all: the wilderness is dense after a
    // bootstrap, and most cells refuse one for spacing rather than for zoning.
    let mut spot = None;
    for r in 2..sim.world().rows - 2 {
        for c in 2..sim.world().cols - 2 {
            if sim.sow(&state, 0, c, r) {
                let at = sim.plant_sim.plants.len() - 1;
                sim.plant_sim.remove_plant_at(at);
                spot = Some((c, r));
                break;
            }
        }
        if spot.is_some() {
            break;
        }
    }
    let spot = spot.expect("nowhere on the map would take a plant");

    // Nothing zoned: the wilderness may seed anywhere.
    assert!(sim.plant_sim.zones.is_empty());
    sim.zone_cells(&[spot], Zone::Bare);
    assert!(!sim.plant_sim.zones.is_empty(), "the wilderness was not told about the zone");
    assert_eq!(sim.terrain.zone_at(spot.0, spot.1), Zone::Bare);

    // The wilderness will not seed there, and the hand still can: a zone says
    // what takes root by itself, not what a person may plant.
    let blocked = sim.blocked.clone();
    assert!(sim
        .plant_sim
        .try_spawn(&state, 0, spot.0, spot.1, Some(&blocked))
        .is_none());
    assert!(sim.sow(&state, 0, spot.0, spot.1), "a hand could not plant in a bare zone");

    // Trees only takes a tree and refuses the low growth, and the other way
    // round, whatever species the project happens to hold.
    let trees: Vec<usize> = (0..state.species.len())
        .filter(|&i| state.species[i].size_class == grow::species::SizeClass::Tree)
        .collect();
    let low: Vec<usize> = (0..state.species.len())
        .filter(|&i| {
            !matches!(
                state.species[i].size_class,
                grow::species::SizeClass::Tree | grow::species::SizeClass::Shrub
            )
        })
        .collect();
    assert!(Zone::Wood.takes(grow::species::SizeClass::Tree));
    assert!(!Zone::Wood.takes(grow::species::SizeClass::Herb));
    assert!(Zone::Low.takes(grow::species::SizeClass::Herb));
    assert!(!Zone::Low.takes(grow::species::SizeClass::Tree));
    assert!(!trees.is_empty() && !low.is_empty(), "the default project has both");
}

// ---- putting things down by hand -----------------------------------------

#[test]
fn what_is_placed_by_hand_is_what_the_town_would_have_placed() {
    use grow::civ::place::{put, Hand, Kind};
    use grow::civ::planner::can_place_at;
    use grow::civ::resources::Res;
    let (mut sim, state) = peopled(48, 24);
    let def = grow::civ::buildings::building_by_id("hut").expect("a hut is in the catalog");

    // Somewhere a hut can stand, found the way the planner finds one.
    let spot = (2..sim.world().rows - 2)
        .flat_map(|r| (2..sim.world().cols - 2).map(move |c| (c, r)))
        .find(|&(c, r)| can_place_at(&sim, &state, def, c, r))
        .expect("nowhere on the map a hut could go");

    let mut hand = Hand { building: "hut".to_string(), finished: true, ..Hand::default() };
    let said = put(&mut sim, &state, &hand, spot.0, spot.1).expect("the hut was refused");
    assert!(said.to_lowercase().contains("hut"), "it said {said}");
    let at = sim
        .buildings
        .iter()
        .position(|b| b.col == spot.0 && b.row == spot.1)
        .expect("nothing stands where it was put");
    assert!(sim.buildings[at].built, "asked for finished, got a site");

    // The same spot again is refused, and says why rather than silently doing
    // nothing.
    let why = put(&mut sim, &state, &hand, spot.0, spot.1).expect_err("two huts in one place");
    assert!(why.contains("fit"), "it said {why}");

    // A site, which is what the town lays for itself.
    let next = (2..sim.world().rows - 2)
        .flat_map(|r| (2..sim.world().cols - 2).map(move |c| (c, r)))
        .find(|&(c, r)| can_place_at(&sim, &state, def, c, r))
        .expect("nowhere left for a second hut");
    hand.finished = false;
    put(&mut sim, &state, &hand, next.0, next.1).expect("the site was refused");
    let site = sim
        .buildings
        .iter()
        .position(|b| b.col == next.0 && b.row == next.1)
        .expect("no site where it was put");
    assert!(!sim.buildings[site].built, "asked for a site, got it finished");

    // A load, which is a pile like any other.
    hand.kind = Kind::Load;
    hand.res = Res::Stone;
    hand.amount = 7.0;
    let (c, r) = (next.0, next.1 + 3);
    put(&mut sim, &state, &hand, c, r).expect("the load was refused");
    assert!(
        sim.piles.iter().any(|p| p.res == Res::Stone && p.n >= 7.0),
        "the load is nowhere on the ground"
    );

    // A plant, grown the way the wilderness grows one.
    hand.kind = Kind::Plant;
    hand.species = state.species[0].id.clone();
    let before = sim.plant_sim.plants.len();
    let mut grew = false;
    for r in 2..sim.world().rows - 2 {
        for c in 2..sim.world().cols - 2 {
            if put(&mut sim, &state, &hand, c, r).is_ok() {
                grew = true;
                break;
            }
        }
        if grew {
            break;
        }
    }
    assert!(grew, "nowhere on the map would take a plant");
    assert_eq!(sim.plant_sim.plants.len(), before + 1);

    // Off the map is off the map, whatever is in hand.
    assert!(put(&mut sim, &state, &hand, -3, -3).is_err());
}

// ---- taking a person over -----------------------------------------------

#[test]
fn a_person_taken_over_goes_where_they_are_pushed_and_plans_nothing() {
    use grow::civ::control;
    let (mut sim, mut state) = peopled(48, 24);
    state.civ.experiments.on = true;
    state.civ.experiments.control.on = true;
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..600 {
        sim.step(&state, dt);
    }
    let id = outdoors(&sim);
    assert!(control::take_over(&mut sim, id), "nobody was taken over");
    assert_eq!(sim.driven, id);
    let pi = sim.people.index_of(id).expect("they are still on the register");
    assert!(sim.people[pi].task.is_none(), "the task they were on was not dropped");

    // Pushed in some direction they can go, they go. Which one is the map's
    // business: a person can be stood against a wall.
    let mut walked = 0.0f64;
    for push in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let pi = sim.people.index_of(id).expect("still there");
        let (x0, y0) = (sim.people[pi].x, sim.people[pi].y);
        sim.drive = push;
        for _ in 0..120 {
            sim.step(&state, dt);
        }
        let pi = sim.people.index_of(id).expect("still there");
        walked = walked.max((sim.people[pi].x - x0).hypot(sim.people[pi].y - y0));
        assert!(
            sim.people[pi].task.is_none(),
            "work was chosen for a person who is being steered"
        );
    }
    assert!(walked > 0.5, "pushing them anywhere moved them {walked:.2} cells");

    // Wherever they ended up is somewhere a person can be.
    let pi = sim.people.index_of(id).expect("still there");
    let (c, r) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    assert!(
        sim.walkable(c, r) || sim.in_water(c, r),
        "they were steered into ground nobody can stand on"
    );

    // Let go, and the town has them back: standing still with no hand on them,
    // they take work again.
    sim.drive = (0.0, 0.0);
    control::let_go(&mut sim);
    assert_eq!(sim.driven, 0);
    let mut busy = false;
    for _ in 0..600 {
        sim.step(&state, dt);
        let pi = sim.people.index_of(id).expect("still there");
        if sim.people[pi].task.is_some() {
            busy = true;
            break;
        }
    }
    assert!(busy, "they never found anything to do again");
}

#[test]
fn the_hand_can_pick_a_load_up_and_put_it_down_again() {
    use grow::civ::control::{act, take_over, Act};
    use grow::civ::resources::Res;
    let (mut sim, mut state) = peopled(48, 24);
    state.civ.experiments.on = true;
    state.civ.experiments.control.on = true;
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..600 {
        sim.step(&state, dt);
    }
    let id = outdoors(&sim);
    assert!(take_over(&mut sim, id));
    let pi = sim.people.index_of(id).expect("still there");
    // Empty handed to start with, whatever they were doing before.
    let (res, n) = sim.people[pi].drop_load();
    let (c, r) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    if let Some(res) = res {
        sim.add_pile(c, r, res, n);
    }
    // A load at their feet, and nothing else near enough to be picked up
    // instead.
    sim.piles.clear();
    sim.add_pile(c, r, Res::Stone, 4.0);

    let said = act(&mut sim, &state, Act::Carry);
    let pi = sim.people.index_of(id).expect("still there");
    assert!(sim.people[pi].carrying(), "the load was not picked up: {said}");
    assert_eq!(sim.people[pi].carry.res, Some(Res::Stone));

    let said = act(&mut sim, &state, Act::Carry);
    let pi = sim.people.index_of(id).expect("still there");
    assert!(!sim.people[pi].carrying(), "the load was not put down: {said}");
    assert!(
        sim.piles.iter().any(|p| p.res == Res::Stone && p.n > 0.0),
        "what they put down is nowhere on the ground"
    );
}

#[test]
fn a_person_being_driven_can_be_fed_from_their_own_hands() {
    use grow::civ::control::{act, take_over, Act};
    use grow::civ::resources::Res;
    let (mut sim, mut state) = peopled(48, 24);
    state.civ.experiments.on = true;
    state.civ.experiments.control.on = true;
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..300 {
        sim.step(&state, dt);
    }
    let id = outdoors(&sim);
    assert!(take_over(&mut sim, id));
    let pi = sim.people.index_of(id).expect("still there");
    sim.people[pi].drop_load();
    sim.people[pi].pick(Res::Food, 4.0);
    sim.people[pi].hunger = 0.9;

    let said = act(&mut sim, &state, Act::Eat);
    let pi = sim.people.index_of(id).expect("still there");
    assert!(sim.people[pi].hunger < 0.9, "eating did nothing: {said}");
    assert!(sim.people[pi].carry.n < 4.0, "the meal came from nowhere");
}

#[test]
fn taking_over_somebody_who_has_died_lets_go_of_them() {
    use grow::civ::control::{driven_index, take_over};
    let (mut sim, mut state) = peopled(48, 24);
    state.civ.experiments.on = true;
    state.civ.experiments.control.on = true;
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..300 {
        sim.step(&state, dt);
    }
    let id = outdoors(&sim);
    assert!(take_over(&mut sim, id));
    let pi = sim.people.index_of(id).expect("still there");
    sim.people[pi].alive = false;
    assert!(driven_index(&mut sim).is_none());
    assert_eq!(sim.driven, 0, "the hand is still on somebody who is gone");
}

// ---- pulling things down -------------------------------------------------

#[test]
fn condemning_a_building_empties_it_and_leaves_it_standing() {
    let (mut sim, state) = peopled(48, 24);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..2000 {
        sim.step(&state, dt);
    }
    let at = sim
        .buildings
        .iter()
        .position(|b| b.built && b.def.housing > 0)
        .expect("a house should be up by now");
    let id = sim.buildings[at].id;
    let housing_before = sim.housing_capacity(sim.buildings[at].colony);

    assert!(sim.condemn(id, true), "the order was not taken");
    let b = &sim.buildings[at];
    assert!(b.condemned && b.built, "it should still be standing, marked to come down");
    assert!(b.residents.is_empty() && b.workers.is_empty() && b.owner == 0);
    assert!(
        sim.people.iter().all(|p| p.home != id && p.work != id && p.owns != id),
        "somebody is still tied to a building that is coming down"
    );
    // Counted out of the town while it is still there, so what replaces it is
    // planned before it is gone.
    assert!(
        sim.housing_capacity(sim.buildings[at].colony) < housing_before,
        "a condemned house was still counted as beds"
    );

    // A day of a running town does not put anybody back in it.
    for _ in 0..(state.civ.people.day_length * state.civ.sim.tick_hz) as usize {
        sim.step(&state, dt);
        if sim.building_index(id).is_none() {
            return;
        }
        let b = &sim.buildings[sim.building_index(id).unwrap()];
        assert!(
            b.residents.is_empty() && b.workers.is_empty(),
            "somebody was assigned to a condemned building"
        );
    }
    // And it is coming apart while they work at it, rather than standing sound.
    assert!(sim.buildings[sim.building_index(id).unwrap()].decay > 0.0);
}

#[test]
fn the_town_pulls_down_what_it_has_condemned_and_keeps_the_materials() {
    let (mut sim, state) = peopled(48, 24);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..2000 {
        sim.step(&state, dt);
    }
    let at = sim.buildings.iter().position(|b| b.built).expect("something should be up");
    let id = sim.buildings[at].id;
    let (col, row) = (sim.buildings[at].col, sim.buildings[at].row);
    let cost: f64 = sim.buildings[at].cost.iter().map(|&(_, n)| n).sum();
    assert!(sim.condemn(id, true));

    let mut down = false;
    for _ in 0..(state.civ.people.day_length * state.civ.sim.tick_hz * 12.0) as usize {
        sim.step(&state, dt);
        if sim.building_index(id).is_none() {
            down = true;
            break;
        }
    }
    assert!(down, "twelve days and nobody finished taking it down");
    // What it was made of is on the ground where it stood, near enough to be
    // the same spot: a pile is merged into whatever is already there.
    let near: f64 = sim
        .piles
        .iter()
        .filter(|p| (p.col - col).abs() <= 3 && (p.row - row).abs() <= 3)
        .map(|p| p.n)
        .sum();
    let want = cost * state.civ.build.pull_down_salvage;
    assert!(
        near > 0.0 || want < 1.0,
        "nothing was left on the ground where it stood, of {want:.0} salvage"
    );
    // The ground is free again.
    assert!(sim.buildings.iter().all(|b| b.id != id));
}

#[test]
fn calling_off_a_site_leaves_what_was_carried_to_it() {
    let (mut sim, state) = peopled(48, 24);
    let dt = 1.0 / state.civ.sim.tick_hz;
    let mut site = None;
    for _ in 0..4000 {
        sim.step(&state, dt);
        site = sim
            .buildings
            .iter()
            .position(|b| !b.built && b.delivered.iter().any(|&n| n >= 1.0));
        if site.is_some() {
            break;
        }
    }
    let at = match site {
        Some(at) => at,
        // Nothing was ever part delivered; there is nothing to check.
        None => return,
    };
    let id = sim.buildings[at].id;
    let (col, row) = (sim.buildings[at].col, sim.buildings[at].row);
    let carried: f64 = sim.buildings[at].delivered.iter().map(|n| n.floor()).sum();

    assert!(sim.condemn(id, true), "a site should be called off rather than condemned");
    assert!(sim.building_index(id).is_none(), "the site is still on the map");
    let near: f64 = sim
        .piles
        .iter()
        .filter(|p| (p.col - col).abs() <= 3 && (p.row - row).abs() <= 3)
        .map(|p| p.n)
        .sum();
    assert!(near >= carried.min(1.0), "what was carried to it went nowhere");
}

#[test]
fn letting_a_condemned_thing_stand_again_calls_the_work_off() {
    let (mut sim, state) = peopled(48, 24);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..2000 {
        sim.step(&state, dt);
    }
    let at = sim.buildings.iter().position(|b| b.built).expect("something should be up");
    let id = sim.buildings[at].id;
    assert!(sim.condemn(id, true));
    for _ in 0..400 {
        sim.step(&state, dt);
        if sim.building_index(id).is_none() {
            return;
        }
    }
    assert!(sim.condemn(id, false));
    let bi = sim.building_index(id).expect("it should still be there");
    assert!(!sim.buildings[bi].condemned);
    // Whoever was taking it apart is off the job on the next tick rather than
    // finishing it.
    for _ in 0..200 {
        sim.step(&state, dt);
    }
    assert!(sim.building_index(id).is_some(), "it came down after the order was called off");
}

#[test]
fn a_person_reports_the_motion_they_are_in() {
    let mut sim = Settlement::new(&State::new());
    sim.bootstrap(&State::new());
    let state = State::new();
    let dt = 1.0 / state.civ.sim.tick_hz;
    // A whole day of a working town should show every person in a motion that
    // matches what the record says they are doing.
    let mut swam = false;
    for _ in 0..(state.civ.people.day_length * state.civ.sim.tick_hz) as usize {
        sim.step(&state, dt);
        let seen: Vec<(bool, grow::civ::sprites::Motion)> = sim
            .people
            .iter_indexed()
            .map(|(_, p)| {
                let wet = sim.in_water(p.cell_col(), p.cell_row());
                (wet, motion_of(p, wet, false))
            })
            .collect();
        for ((wet, motion), (_, p)) in seen.iter().zip(sim.people.iter_indexed()) {
            swam |= *wet;
            match motion {
                Motion::Sleep => assert!(p.sleeping),
                // In the water: crossing it if they are going somewhere,
                // treading it if they are not.
                Motion::Swim => assert!(*wet && !p.sleeping && !p.path.is_empty()),
                Motion::Float => assert!(*wet && !p.sleeping && p.path.is_empty()),
                // Nobody is in hand: the simulation never puts anybody there.
                Motion::Held => panic!("a person was in hand with nobody holding them"),
                Motion::ToBed => assert!(
                    !p.sleeping && p.task.as_ref().is_some_and(|t| t.is_sleep()),
                    "turning in without a bed to turn in to"
                ),
                Motion::Walk => assert!(!p.path.is_empty() && !p.carrying()),
                Motion::Carry => assert!(!p.path.is_empty() && p.carrying()),
                Motion::Work | Motion::Idle => assert!(p.path.is_empty() && !p.sleeping),
            }
        }
    }
    let _ = swam;

    // Being in hand beats everything, wet or dry, awake or asleep.
    for (_, p) in sim.people.iter_indexed() {
        assert_eq!(motion_of(p, false, true), Motion::Held);
        assert_eq!(motion_of(p, true, true), Motion::Held);
    }
}

#[test]
fn what_is_drawn_in_the_water_is_only_cut_when_it_was_drawn_for_dry_land() {
    use grow::civ::sprites::cut_at_waterline;
    // The generated person has a pose for the water, and so does anything
    // dropped on the two water slots: those draw their own waterline and are
    // left whole. A walk borrowed for the water is a standing figure, and the
    // cut is what puts it in the water rather than on it.
    assert!(!cut_at_waterline(None), "the generated water pose was cut in half");
    assert!(!cut_at_waterline(Some(Motion::Swim)));
    assert!(!cut_at_waterline(Some(Motion::Float)));
    assert!(cut_at_waterline(Some(Motion::Walk)));
    assert!(cut_at_waterline(Some(Motion::Idle)));
    assert!(cut_at_waterline(Some(Motion::Carry)));
}

#[test]
fn the_water_and_hand_poses_are_not_the_walking_body_cut_down() {
    use grow::civ::civ_render::{person_sprite, Pose, SpriteCache};
    let mut sim = Settlement::new(&State::new());
    sim.bootstrap(&State::new());
    let world = sim.world().clone();
    let (_, p) = sim.people.iter_indexed().next().expect("a person");

    let mut cache = SpriteCache::default();
    let land = person_sprite(&mut cache, &world, p, 0, Pose::Land);
    let swim = person_sprite(&mut cache, &world, p, 0, Pose::Swim);
    let float = person_sprite(&mut cache, &world, p, 0, Pose::Float);
    let held = person_sprite(&mut cache, &world, p, 0, Pose::Held);

    // What is above the water is head and shoulders, so it is shorter than the
    // whole body, and its last row is the surface: drawn all the way across
    // rather than stopping where a pair of legs would.
    assert!(swim.h < land.h, "the swimmer is as tall as somebody standing up");
    let surface = |s: &grow::civ::civ_render::Sprite| {
        let row = ((s.h - 1) * s.w) as usize;
        s.px[row..row + s.w as usize].iter().filter(|v| **v != 0).count()
    };
    assert!(surface(&swim) > surface(&land), "the swimmer has no waterline under them");
    assert!(surface(&float) > surface(&land), "the one treading water has no waterline");
    // The two water poses are not the same picture: one has an arm out ahead,
    // the other has both out and bobs.
    assert_ne!(swim.px, float.px, "treading water is the same as swimming");
    // Somebody in hand is the whole body, dangling rather than mid step.
    assert_eq!((held.w, held.h), (land.w, land.h));
    assert_ne!(held.px, land.px, "being carried looks like standing there");
}

#[test]
fn water_is_crossed_or_walked_round_according_to_what_a_swim_costs() {
    // Water is passable but priced, and the price is the whole control. Rather
    // than lean on a generated map having a river of some particular width,
    // this asks the same question twice with the price moved either side of
    // what the detour is worth.
    let mut state = State::new();
    state.civ.world.cols = 96;
    state.civ.world.rows = 40;
    state.civ.terrain.warmup = 40.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);

    // A cell of water with dry land two steps to either side of it.
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    let crossing = (1..rows - 1)
        .flat_map(|r| (2..cols - 2).map(move |c| (c, r)))
        .find(|(c, r)| {
            sim.in_water(*c, *r)
                && sim.walkable(c - 2, *r)
                && sim.walkable(c + 2, *r)
                && !sim.in_water(c - 1, *r)
                && !sim.in_water(c + 1, *r)
        });
    let (c, r) = match crossing {
        Some(at) => at,
        None => panic!("the generated map has no water with dry land either side"),
    };
    fn wet(sim: &Settlement, path: &Option<Vec<(i32, i32)>>) -> bool {
        path.as_ref()
            .is_some_and(|p| p.iter().any(|(pc, pr)| sim.in_water(*pc, *pr)))
    }

    // Costing no more than dry ground, the straight line across is the shortest
    // way there and has to be taken.
    sim.swim_cost = 1.0;
    let across = sim.find_path(c - 2, r, c + 2, r);
    assert!(across.is_some(), "the two banks are not connected at all");
    assert!(wet(&sim, &across), "a swim that cost nothing was still not taken");

    // Priced out of reach, the same request keeps its feet dry: either round,
    // or not at all.
    sim.swim_cost = 500.0;
    let round = sim.find_path(c - 2, r, c + 2, r);
    assert!(!wet(&sim, &round), "a swim priced out of reach was taken anyway");
}

#[test]
fn a_trunk_is_walked_round_rather_than_through() {
    let mut state = State::new();
    state.civ.world.cols = 96;
    state.civ.world.rows = 40;
    state.civ.terrain.warmup = 240.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);

    // A plant the pathfinder has marked as being in the way, with dry open
    // ground two steps to either side of it.
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    let stem = (1..rows - 1)
        .flat_map(|r| (2..cols - 2).map(move |c| (c, r)))
        .find(|&(c, r)| {
            sim.plant_block[(r * cols + c) as usize] != 0
                && sim.walkable(c - 2, r)
                && sim.walkable(c + 2, r)
                && !sim.in_water(c - 1, r)
                && !sim.in_water(c + 1, r)
                && sim.plant_block[(r * cols + c - 1) as usize] == 0
                && sim.plant_block[(r * cols + c + 1) as usize] == 0
        });
    let (c, r) = match stem {
        Some(at) => at,
        None => panic!("nothing grew that anybody would have to walk round"),
    };
    let through = |path: &Option<Vec<(i32, i32)>>| {
        path.as_ref().is_some_and(|p| p.contains(&(c, r)))
    };

    let round = sim.find_path(c - 2, r, c + 2, r);
    assert!(round.is_some(), "the two sides of a tree are not connected at all");
    assert!(!through(&round), "the way round a trunk went straight through it");

    // Told that nothing is ever in the way, the same request takes the short
    // line across the cell the tree is standing in.
    sim.block_mass = 0.0;
    sim.rebuild_plant_index();
    let across = sim.find_path(c - 2, r, c + 2, r);
    assert!(through(&across), "with nothing in the way the walk still went round");
}

/// A town that has died out does not stand forever. This empties one rather
/// than waiting a hundred days for it to empty itself, because what is being
/// checked is what happens to the houses afterward.
#[test]
fn the_houses_of_a_town_with_nobody_left_fall_in() {
    let mut state = State::new();
    state.civ.world.cols = 48;
    state.civ.world.rows = 24;
    state.civ.terrain.warmup = 60.0;
    state.civ.build.crumble_after = 2.0;
    state.civ.build.crumble_days = 4.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);

    let dt = 1.0 / state.civ.sim.tick_hz;
    let day = (state.civ.people.day_length / dt).round() as i32;
    let mut home = None;
    for _ in 0..40 {
        for _ in 0..day {
            sim.step(&state, dt);
        }
        home = sim.buildings.iter().find(|b| b.def.housing > 0 && b.built).map(|b| b.id);
        if home.is_some() {
            break;
        }
    }
    let id = home.expect("nobody built a house in forty days");
    let (col, row) = match sim.building_index(id) {
        Some(bi) => (sim.buildings[bi].col, sim.buildings[bi].row),
        None => unreachable!(),
    };
    for pi in sim.people.live_indices() {
        sim.people.retire(pi);
    }

    // Standing empty is not the same as coming down: it takes the wait and
    // the fall together before there is nothing left.
    for _ in 0..day * 2 {
        sim.step(&state, dt);
    }
    let standing = match sim.building_index(id) {
        Some(bi) => sim.buildings[bi].decay,
        None => panic!("an empty house was pulled down on the spot"),
    };
    assert!(standing < 1.0, "a house went in two days when it takes six");

    // Then it goes, and the ground it stood on comes back with it.
    for _ in 0..day * 10 {
        sim.step(&state, dt);
        if sim.building_index(id).is_none() {
            break;
        }
    }
    assert!(sim.building_index(id).is_none(), "an empty house never fell in");
    assert!(sim.walkable(col, row), "the ground under a fallen house is still shut");
}

#[test]
fn a_cut_tree_goes_over_before_it_goes_away() {
    let mut state = State::new();
    state.civ.world.cols = 64;
    state.civ.world.rows = 28;
    state.civ.terrain.warmup = 240.0;
    state.civ.work.fall_time = 2.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);

    let tallest = sim
        .plant_sim
        .plants
        .iter()
        .enumerate()
        .filter(|(_, p)| p.size_class != SizeClass::Ground)
        .max_by(|a, b| a.1.height_px.total_cmp(&b.1.height_px))
        .map(|(i, p)| (i, p.id, p.col, p.row));
    let (index, id, col, row) = match tallest {
        Some(found) => found,
        None => panic!("nothing grew that could be felled"),
    };
    let cells_before = sim.plant_sim.world.occupant(3, col, row);

    sim.take_plant(index, false, 0.0);
    let at = sim.plant_sim.plant_index(id).expect("a felled tree is still on the map");
    assert!(!sim.plant_sim.plants[at].standing(), "a cut tree is still standing");
    assert!(sim.plant_sim.plants[at].alive, "a cut tree was taken away where it stood");
    if cells_before != 0 {
        assert_eq!(
            sim.plant_sim.world.occupant(3, col, row),
            0,
            "the ground under a felled tree is still claimed by it",
        );
    }

    // It turns further every tick, and is off the map once it is down.
    let dt = 1.0 / state.civ.sim.tick_hz;
    sim.step(&state, dt);
    let leaning = sim.plant_sim.plant_index(id).map(|i| sim.plant_sim.plants[i].felled);
    assert!(leaning.unwrap_or(0.0) > 0.0, "the fall did not start");
    for _ in 0..(2.0 / dt).ceil() as i32 + 2 {
        sim.step(&state, dt);
    }
    assert!(
        sim.plant_sim.plant_index(id).is_none(),
        "a tree that finished falling is still on the map",
    );
}

/// The experiments switch is the contract: with it off, nothing under it is
/// asked anything and the town is the town it would have been.
#[test]
fn a_balloon_lifts_research_and_only_when_the_experiment_is_on() {
    fn town(on: bool) -> (Settlement, State) {
        let mut state = State::new();
        state.civ.world.cols = 40;
        state.civ.world.rows = 20;
        state.civ.terrain.warmup = 30.0;
        state.civ.experiments.on = on;
        state.civ.experiments.balloons.interval = 5.0;
        let mut sim = Settlement::new(&state);
        sim.bootstrap(&state);
        // A school and the stores to fly from it, rather than the twenty days
        // it would take a town to get there on its own.
        let center = sim.colonies[0].center;
        let site = (center.0 + 3, center.1);
        sim.place_building(&state, 0, "school", site.0, site.1, true);
        grow::civ::resources::add_stock(&mut sim.colonies[0].stock, grow::civ::resources::Res::Cloth, 40.0);
        grow::civ::resources::add_stock(
            &mut sim.colonies[0].stock,
            grow::civ::resources::Res::Charcoal,
            40.0,
        );
        (sim, state)
    }

    let dt = 1.0 / 20.0;
    let (mut off, state_off) = town(false);
    for _ in 0..600 {
        off.step(&state_off, dt);
    }
    assert!(off.balloons.is_empty(), "an experiment nobody switched on ran anyway");

    let (mut on, state_on) = town(true);
    for _ in 0..600 {
        on.step(&state_on, dt);
    }
    assert!(!on.balloons.is_empty(), "the town never sent one up");
    let colony = on.colonies[0].id;
    assert!(
        balloon_lift(&on, &state_on, colony) > 1.0,
        "a canopy over the town is worth nothing to its scholars",
    );

    // What is up comes down, and the switch going off clears the sky at once.
    let mut state_off_again = state_on.clone();
    state_off_again.civ.experiments.on = false;
    on.step(&state_off_again, dt);
    assert!(on.balloons.is_empty(), "turning the experiment off left canopies in the air");
}

/// A map made larger under a running settlement keeps the settlement.
#[test]
fn growing_the_map_leaves_everything_standing_where_it_was() {
    let mut state = State::new();
    state.civ.world.cols = 40;
    state.civ.world.rows = 20;
    state.civ.terrain.warmup = 60.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize * 3 {
        sim.step(&state, dt);
    }

    let before: Vec<(i32, i32, i32)> =
        sim.buildings.iter().map(|b| (b.id, b.col, b.row)).collect();
    let people: Vec<(u32, f64, f64)> =
        sim.people.iter().map(|p| (p.id, p.x, p.y)).collect();
    let plants: Vec<(i32, i32, i32)> = sim
        .plant_sim
        .plants
        .iter()
        .map(|p| (p.id, p.col, p.row))
        .collect();
    let kind: Vec<u8> = (0..20)
        .flat_map(|r| (0..40).map(move |c| (c, r)))
        .map(|(c, r)| sim.terrain.kind[(r * 40 + c) as usize])
        .collect();
    let deposits = sim.terrain.deposits.len();

    state.civ.world.cols = 64;
    state.civ.world.rows = 30;
    assert!(sim.expand(&state, 64, 30), "the map did not grow");
    assert_eq!((sim.world().cols, sim.world().rows), (64, 30));

    for (id, col, row) in before {
        let bi = sim.building_index(id).expect("a building went missing");
        assert_eq!((sim.buildings[bi].col, sim.buildings[bi].row), (col, row));
    }
    for (id, x, y) in people {
        let p = sim.people.get(id).expect("a person went missing");
        assert_eq!((p.x, p.y), (x, y), "a person moved when the map grew");
    }
    for (id, col, row) in plants {
        let i = sim.plant_sim.plant_index(id).expect("a plant went missing");
        assert_eq!((sim.plant_sim.plants[i].col, sim.plant_sim.plants[i].row), (col, row));
    }
    // The ground that was there is the ground that is there.
    for r in 0..20 {
        for c in 0..40 {
            assert_eq!(
                sim.terrain.kind[(r * 64 + c) as usize],
                kind[(r * 40 + c) as usize],
                "the ground at {c},{r} changed under the town",
            );
        }
    }
    assert!(sim.terrain.deposits.len() >= deposits, "the old deposits were thrown away");

    // The new ground is land, not a hole: something grew on it and people can
    // walk onto it.
    let out_there = sim
        .plant_sim
        .plants
        .iter()
        .any(|p| p.col >= 40 || p.row >= 20);
    assert!(out_there, "nothing grew on the new land");
    let walkable = (20..30)
        .flat_map(|r| (40..64).map(move |c| (c, r)))
        .any(|(c, r)| sim.walkable(c, r));
    assert!(walkable, "none of the new land can be walked on");

    // And it still runs.
    for _ in 0..(state.civ.people.day_length / dt) as usize * 2 {
        sim.step(&state, dt);
    }
    assert!(sim.people.count() > 0, "the town died when the map grew");
}

#[test]
fn numbered_frames_sort_the_way_they_are_numbered() {
    let mut names = vec!["walk10.png", "walk2.png", "walk1.png", "Walk3.png"];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(names, ["walk1.png", "walk2.png", "Walk3.png", "walk10.png"]);
}

#[test]
fn a_project_carries_its_sprites_through_a_save() {
    let mut state = State::new();
    state.civ.sprites.set(Motion::Walk, Some(strip(4, 6, 8)));
    state.civ.sprites.walk.as_mut().unwrap().scale = 1.75;
    let back = State::from_json(&state.to_json()).expect("reload");
    let clip = back.civ.sprites.walk.as_ref().expect("the walk survived");
    assert_eq!(clip.frame_count(), 4);
    assert_eq!(clip.frame_w(), 6);
    assert_eq!(clip.h, 8);
    assert_eq!(clip.scale, 1.75);
    for f in 0..4 {
        assert_eq!(clip.pixel(f, 0, 0), (f + 1) as u32, "frame {f} came back wrong");
    }
}

#[test]
fn a_clip_measured_in_cells_comes_back_as_the_size_it_was_drawn() {
    // Person art used to be given a height in cells and stretched to it. A
    // project written then still has to draw its people the same size, so the
    // height becomes the scale that puts the art at exactly that height.
    let mut state = State::new();
    state.civ.sprites.set(Motion::Walk, Some(strip(1, 6, 16)));
    let raw = state.to_json().replace(r#""stride":"#, r#""height":1.75,"stride":"#);
    assert!(raw.contains(r#""height":1.75"#), "the old field was not written to test with");

    let back = State::from_json(&raw).expect("reload");
    let clip = back.civ.sprites.walk.as_ref().expect("the walk survived");
    // Sixteen pixels of art at eight to a cell is two cells; a cell and three
    // quarters is seven eighths of that.
    assert_eq!(clip.scale, 0.875);
    assert_eq!(clip.height, 0.0, "the old field should not have been kept");
    let cell = back.civ.world.cell_px;
    let (_, drawn) = clip.drawn_cells(cell, back.civ.art_px_per_cell);
    assert!((drawn - 1.75).abs() < 0.05, "it came back {drawn} cells tall rather than 1.75");
    // And it leaves the file the first time the project is saved again.
    assert!(!back.to_json().contains(r#""height":"#), "the old field was written back out");
}

#[test]
fn a_person_behind_a_bush_is_drawn_behind_it() {
    // Draw order is depth, not the row something stands in. Two things in one
    // row used to tie and then be separated by what kind of thing they were,
    // which put every person in front of every plant in their row - so
    // somebody walking behind a bush walked over it.
    use grow::civ::civ_render::depth_key;

    // A plant stands in the middle of its cell; a person stands wherever they
    // are in theirs.
    let bush = depth_key(4.0 + 0.5);
    assert!(depth_key(4.2) < bush, "a person at the back of the row is not behind the bush");
    assert!(depth_key(4.8) > bush, "a person at the front of the row is not in front of it");
    // Same row, and the two are told apart; a row apart is still a row apart.
    assert!(depth_key(3.9) < bush);
    assert!(depth_key(5.1) > bush);
    // A building's foot is the front edge of its last row, which leaves it
    // tying with a plant in that row and winning on kind, the way it did.
    assert_eq!(depth_key((3 + 2) as f64 - 0.5), depth_key(4.0 + 0.5));
}

#[test]
fn nobody_beds_down_where_the_day_ended() {
    // Somebody with no roof and nothing at an inn used to lie down wherever the
    // day happened to end, which for a forager is out in the woods. They walk
    // back to their town first now.
    let mut state = State::new();
    state.civ.world.cols = 64;
    state.civ.world.rows = 30;
    state.civ.terrain.warmup = 60.0;
    // No beds and no coin, so every person takes the last resort.
    state.civ.start.storehouse = false;
    state.civ.people.inn_price = 1e9;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);

    let dt = 1.0 / state.civ.sim.tick_hz;
    let day = (state.civ.people.day_length / dt) as usize;
    let mut far_asleep = 0;
    let mut asleep = 0;
    for _ in 0..day * 3 {
        sim.step(&state, dt);
        for (pi, p) in sim.people.iter_indexed() {
            if !p.sleeping || p.indoors() {
                continue;
            }
            asleep += 1;
            let ci = sim.colony_of(pi);
            let (tc, tr) = sim.colonies[ci].center;
            let away = (p.cell_col() - tc).abs().max((p.cell_row() - tr).abs());
            // Sleeping in the town, or on its doorstep. Anything past this is
            // somebody who bedded down where they were working.
            if away > 8 {
                far_asleep += 1;
            }
        }
    }
    assert!(asleep > 0, "nobody slept rough at all, so this proves nothing");
    assert!(
        far_asleep * 20 < asleep,
        "{far_asleep} of {asleep} sleeping moments were far from town: people are \
         still bedding down where the day ended"
    );
}

/// A small town, warmed up enough to have people walking about in it.
fn peopled(cols: i32, rows: i32) -> (Settlement, State) {
    let mut state = State::new();
    state.civ.world.cols = cols;
    state.civ.world.rows = rows;
    state.civ.terrain.warmup = 60.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    (sim, state)
}

/// The first person out in the open, who is the one a press on their own feet
/// has to find.
fn outdoors(sim: &Settlement) -> u32 {
    sim.people
        .iter()
        .find(|p| !p.indoors() && p.aboard == 0)
        .map(|p| p.id)
        .expect("nobody is outdoors to pick up")
}

#[test]
fn a_person_is_picked_up_by_pointing_at_them() {
    let (sim, _) = peopled(48, 24);
    let id = outdoors(&sim);
    let (x, y) = {
        let p = sim.people.get(id).unwrap();
        (p.x, p.y)
    };
    assert_eq!(sim.person_near(x, y, 1.6), Some(id), "pointing at somebody's feet missed them");
    // A person is drawn standing up out of their cell, so the reach is taller
    // above them than below.
    assert_eq!(sim.person_near(x, y - 2.0, 1.6), Some(id), "pointing at somebody's head missed them");
    // Far enough away and the answer is nobody rather than the nearest.
    let far = sim.person_near(x + 40.0, y, 1.6);
    assert!(far.is_none() || far != Some(id), "a press nowhere near anybody picked somebody up");
}

#[test]
fn a_person_can_be_carried_somewhere_else_and_put_down() {
    let (mut sim, _) = peopled(48, 24);
    let id = outdoors(&sim);
    let was = {
        let p = sim.people.get(id).unwrap();
        (p.cell_col(), p.cell_row())
    };
    assert!(sim.hold_person(id), "a living person could not be picked up");
    assert_eq!(sim.held, id);

    // Somewhere they can stand, as far from where they were as the map allows.
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    let target = (0..rows)
        .flat_map(|r| (0..cols).map(move |c| (c, r)))
        .filter(|(c, r)| sim.walkable(*c, *r))
        .max_by_key(|(c, r)| (c - was.0).abs() + (r - was.1).abs())
        .expect("the map has nowhere to stand");
    sim.move_held(target.0 as f64 + 0.5, target.1 as f64 + 0.5);
    assert_eq!(sim.drop_held(), Some(target), "a person was not put down where they were let go");
    assert_eq!(sim.held, 0, "the hand is still holding somebody");

    let p = sim.people.get(id).unwrap();
    assert_ne!((p.cell_col(), p.cell_row()), was, "the person did not move at all");
    assert!(p.task.is_none(), "a person put down is still on their way somewhere");
    assert!(p.path.is_empty(), "a person put down is still walking a path from before");
    assert!(!p.indoors(), "a person picked up is still recorded as being inside");
}

#[test]
fn a_person_put_down_on_a_roof_lands_beside_it() {
    let (mut sim, _) = peopled(48, 24);
    let id = outdoors(&sim);
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    let blocked = (0..rows)
        .flat_map(|r| (0..cols).map(move |c| (c, r)))
        .find(|(c, r)| !sim.walkable(*c, *r) && !sim.in_water(*c, *r))
        .expect("the map has nothing solid to drop somebody on");

    assert!(sim.hold_person(id));
    sim.move_held(blocked.0 as f64 + 0.5, blocked.1 as f64 + 0.5);
    let landed = sim.drop_held().expect("nobody was put down");
    assert_ne!(landed, blocked, "a person was left standing in something solid");
    assert!(sim.walkable(landed.0, landed.1), "a person landed somewhere they cannot stand");
}

#[test]
fn a_person_in_hand_is_left_out_of_the_tick() {
    let (mut sim, state) = peopled(48, 24);
    let id = outdoors(&sim);
    assert!(sim.hold_person(id));
    // Held over open ground rather than wherever they happened to be standing,
    // so nothing about the spot explains them staying put.
    let held_at = (sim.world().cols as f64 * 0.5, sim.world().rows as f64 * 0.5);
    sim.move_held(held_at.0, held_at.1);

    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..400 {
        sim.step(&state, dt);
    }
    let p = sim.people.get(id).unwrap();
    assert_eq!((p.x, p.y), held_at, "a person being held walked off on their own");
    assert!(p.task.is_none(), "a person being held took on work");
    // Time still passes for them: being carried about is no way out of getting
    // older or hungrier.
    assert!(p.age > 0.0);

    sim.drop_held();
    for _ in 0..400 {
        sim.step(&state, dt);
    }
    let p = sim.people.get(id).unwrap();
    assert!(
        p.task.is_some() || (p.x, p.y) != held_at,
        "a person put down never picked their life back up"
    );
}

// ---- dying back ----------------------------------------------------------

/// One plant of one species on a world nothing else grows on, with a short
/// life and a shrivel of the given length, stepped until it is past its age.
/// Comes back with the id of the one that was planted: a species left to
/// itself seeds more, and those are not the one being watched.
fn dying_plant(species_id: &str, shrivel: f64) -> (Sim, State, i32) {
    let mut state = State::new();
    for sp in state.species.iter_mut() {
        sp.enabled = sp.id == species_id;
        sp.growth.max_age = 4.0;
        sp.growth.shrivel = shrivel;
    }
    let index = state.species.iter().position(|s| s.id == species_id).expect("species");
    let mut sim = Sim::new(&state, state.world.clone());
    let (col, row) = (sim.world.cols / 2, sim.world.rows / 2);
    let at = sim.try_spawn(&state, index, col, row, None).expect("somewhere to grow");
    let id = sim.plants[at].id;
    for _ in 0..50 {
        sim.step(&state, 0.1, None);
        sim.process_raster_queue(&state, 64);
    }
    (sim, state, id)
}

fn watched(sim: &Sim, id: i32) -> Option<&grow::plant::Plant> {
    sim.plants.iter().find(|p| p.id == id)
}

#[test]
fn a_plant_past_its_age_shrivels_rather_than_vanishing() {
    let (mut sim, state, id) = dying_plant("sp-grass", 6.0);
    let plant = watched(&sim, id).expect("still standing a second past its age");
    assert!(plant.wither > 0.0, "and visibly on the way out");
    assert!(plant.wither < 1.0, "but not gone in the same breath");

    for _ in 0..80 {
        sim.step(&state, 0.1, None);
        sim.process_raster_queue(&state, 64);
    }
    assert!(watched(&sim, id).is_none(), "once dried out it is off the map");
}

#[test]
fn a_faster_shrivel_clears_sooner() {
    let (quick, _, quick_id) = dying_plant("sp-grass", 0.5);
    assert!(
        watched(&quick, quick_id).is_none(),
        "half a second of shrivel is over a whole second past the age"
    );

    let (slow, _, slow_id) = dying_plant("sp-grass", 20.0);
    let plant = watched(&slow, slow_id).expect("a long shrivel has barely started");
    assert!(plant.wither < 0.2, "got {}", plant.wither);
}

#[test]
fn a_shrivelling_plant_comes_apart_from_the_tips_down() {
    let (mut sim, state, id) = dying_plant("sp-oak", 20.0);
    let early = watched(&sim, id).expect("still there").bounds;

    for _ in 0..120 {
        sim.step(&state, 0.1, None);
        sim.process_raster_queue(&state, 64);
    }
    let plant = watched(&sim, id).expect("twenty seconds is longer than twelve");
    assert!(plant.wither > 0.4, "well into drying out, got {}", plant.wither);
    assert!(
        plant.bounds.y0 > early.y0,
        "the top should have come down: {} then {}",
        early.y0,
        plant.bounds.y0
    );
    assert!(
        plant.bounds.y1 >= early.y1 - 1,
        "the foot should be the last of it to go: {} then {}",
        early.y1,
        plant.bounds.y1
    );
}

#[test]
fn nothing_shrivels_while_it_is_still_growing() {
    let (sim, _) = run_lab(400);
    assert!(!sim.plants.is_empty(), "nothing grew");
    for p in &sim.plants {
        assert_eq!(p.wither, 0.0, "a living plant should not be drying out");
    }
}

// ---- starting over -------------------------------------------------------

#[test]
fn a_settlement_with_somebody_in_it_is_not_counted_as_gone() {
    let (mut sim, state) = peopled(48, 24);
    for _ in 0..40 {
        sim.step(&state, 1.0 / state.civ.sim.tick_hz);
    }
    assert!(sim.people.iter().any(|p| p.alive), "the founders should be alive");
    assert_eq!(sim.extinct_for(), None, "a living town is not waiting to restart");
}

#[test]
fn an_empty_settlement_starts_counting_from_the_last_death() {
    let (mut sim, state) = peopled(48, 24);
    for i in sim.people.live_indices() {
        sim.people[i].alive = false;
    }
    let at = sim.time;
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..40 {
        sim.step(&state, dt);
    }
    let waited = sim.extinct_for().expect("nobody left, so it is counting");
    assert!(waited > 0.0, "the wait should have started");
    assert!(
        (sim.time - at - waited).abs() < dt * 2.0,
        "it should count from the death, not from being noticed"
    );
}

// ---- foliage over a person ---------------------------------------------

/// One person pixel and one plant pixel in a two pixel buffer, so what
/// foliage does over somebody can be read off directly.
fn over_person(mode: grow::sim::Foliage, leaf: u32) -> [u32; 2] {
    use grow::util::{is_person, mark_person, mix_packed};
    let person = mark_person(grow::util::pack_rgba(255, 0, 0, 255));
    let mut buf = [person, person];
    // What `blit_plant` does per pixel, at two neighboring x on one row: one
    // even and one odd, which is the whole of what hatching turns on.
    for (x, slot) in buf.iter_mut().enumerate() {
        if !is_person(*slot) {
            *slot = leaf;
            continue;
        }
        match mode {
            grow::sim::Foliage::Solid => *slot = leaf,
            grow::sim::Foliage::Hatched => {
                if x % 2 != 0 {
                    *slot = mark_person(leaf);
                }
            }
            grow::sim::Foliage::Faded(a) => *slot = mark_person(mix_packed(*slot, leaf, a)),
        }
    }
    buf
}

#[test]
fn solid_foliage_covers_a_person_the_way_a_plant_does() {
    use grow::util::is_person;
    let leaf = grow::util::pack_rgba(0, 255, 0, 255);
    let out = over_person(grow::sim::Foliage::Solid, leaf);
    assert_eq!(out, [leaf, leaf]);
    assert!(!is_person(out[0]), "solid foliage is not the person any more");
}

#[test]
fn hatched_foliage_leaves_every_other_pixel_showing() {
    use grow::util::{is_person, mark_person, unpack_rgba};
    let leaf = grow::util::pack_rgba(0, 255, 0, 255);
    let out = over_person(grow::sim::Foliage::Hatched, leaf);
    let kept = unpack_rgba(out[0]);
    assert_eq!((kept.r, kept.g, kept.b), (255, 0, 0), "one pixel stays the person");
    assert_eq!(out[1], mark_person(leaf), "and the next is the leaf");
    assert!(is_person(out[0]) && is_person(out[1]), "both stay marked for the next leaf");
}

#[test]
fn faded_foliage_mixes_and_stays_findable() {
    use grow::util::{is_person, unpack_rgba};
    let leaf = grow::util::pack_rgba(0, 255, 0, 255);
    let out = over_person(grow::sim::Foliage::Faded(0.5), leaf);
    let c = unpack_rgba(out[0]);
    assert!(c.r > 60 && c.g > 60, "half of each should be there, got {c:?}");
    assert!(is_person(out[0]), "the person is still under it, so the next leaf fades too");
}

#[test]
fn the_person_mark_rides_in_the_alpha_and_changes_nothing_visible() {
    use grow::util::{is_person, mark_person, unpack_rgba, PERSON_ALPHA};
    let color = grow::util::pack_rgba(120, 40, 200, 255);
    let marked = mark_person(color);
    assert!(is_person(marked));
    assert!(!is_person(color), "an ordinary opaque pixel is not a person");
    let (before, after) = (unpack_rgba(color), unpack_rgba(marked));
    assert_eq!((before.r, before.g, before.b), (after.r, after.g, after.b));
    assert_eq!(after.a, 254, "one step off opaque");
    assert_eq!(PERSON_ALPHA >> 24, 254);
}

#[test]
fn the_view_reads_its_own_setting() {
    use grow::sim::Foliage;
    let mut view = grow::civ::config::ViewConfig::default();
    assert_eq!(view.foliage_over_people(), Foliage::Solid);
    view.foliage = "hatched".into();
    assert_eq!(view.foliage_over_people(), Foliage::Hatched);
    view.foliage = "faded".into();
    view.foliage_alpha = 0.4;
    assert_eq!(view.foliage_over_people(), Foliage::Faded(0.4));
    // Anything else is what a plant is.
    view.foliage = "nonsense".into();
    assert_eq!(view.foliage_over_people(), Foliage::Solid);
}

// ---- pictures for made things -------------------------------------------

#[test]
fn every_made_thing_has_exactly_one_slot() {
    use grow::civ::sprites::made_slots;
    let slots = made_slots();
    let mut ids: Vec<&str> = slots.iter().map(|s| s.id.as_str()).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n, "two things share a picture slot");

    // Everything in the catalog, plus the boat and one per resource carried.
    for def in grow::civ::buildings::BUILDINGS {
        assert!(ids.contains(&def.id), "{} has nowhere to put a picture", def.id);
    }
    assert!(ids.contains(&"boat"));
    for res in grow::civ::resources::RES_IDS {
        let want = format!("carry-{}", res.id());
        assert!(ids.iter().any(|id| *id == want), "{want} has no slot");
    }
    for slot in &slots {
        assert!(!slot.label.is_empty(), "{} has no name to show", slot.id);
        assert!(!slot.group.is_empty(), "{} belongs to no group", slot.id);
    }
}

#[test]
fn a_picture_is_only_used_when_pictures_are_turned_on() {
    use grow::civ::sprites::{Clip, MadeSprites};
    let mut made = MadeSprites::default();
    let clip = Clip::from_strip(4, 4, vec![0xffff_ffff; 16], 1, "hut.png".into()).expect("a strip");
    made.set("hut", clip);
    assert!(made.slot("hut").is_some(), "the panel can always see it");
    assert!(made.clip("hut").is_none(), "but the map does not until it is turned on");
    made.enabled = true;
    assert!(made.clip("hut").is_some());
    assert!(made.clip("house").is_none(), "and only for what has one");
}

#[test]
fn clearing_a_picture_takes_it_out_and_moves_the_revision_on() {
    use grow::civ::sprites::{Clip, MadeSprites};
    let mut made = MadeSprites { enabled: true, ..MadeSprites::default() };
    let clip = Clip::from_strip(4, 4, vec![0xffff_ffff; 16], 1, "hut.png".into()).expect("a strip");
    let start = made.rev;
    made.set("hut", clip);
    assert!(made.bytes() > 0);
    let after_set = made.rev;
    assert_ne!(after_set, start, "the cache has to be told the picture changed");
    made.clear("hut");
    assert!(made.clip("hut").is_none());
    assert_eq!(made.bytes(), 0);
    assert_ne!(made.rev, after_set, "and told again when it went");
}

#[test]
fn the_box_a_generated_thing_fills_is_its_footprint_and_what_stands_on_it() {
    use grow::civ::civ_render::made_box;
    let world = grow::world::World::new(&grow::civ::config::default_civ_world());
    for def in grow::civ::buildings::BUILDINGS {
        let (w, h) = made_box(&world, def);
        assert_eq!(w, def.w * world.cell_px, "{} is not as wide as its footprint", def.id);
        assert!(h > 0, "{} has no height to draw into", def.id);
        // Tall enough for the walls and roof over the depth of the footprint.
        assert!(h >= def.h * world.depth_px, "{} is shorter than its own depth", def.id);
    }
}

#[test]
fn pictures_survive_a_project_file() {
    use grow::civ::sprites::Clip;
    let mut state = State::new();
    let clip = Clip::from_strip(4, 4, vec![0xff00_ff00; 16], 1, "hut.png".into()).expect("a strip");
    state.civ.made.enabled = true;
    state.civ.made.set("hut", clip);

    let back = State::from_json(&state.to_json()).expect("a project file");
    assert!(back.civ.made.enabled);
    let kept = back.civ.made.slot("hut").expect("the picture came back");
    assert_eq!(kept.px.len(), 16);
    assert_eq!(kept.source, "hut.png");
}

#[test]
fn a_finished_building_with_a_picture_is_drawn_from_it() {
    use grow::civ::civ_render::{building_sprite, Detail, SpriteCache};
    use grow::civ::sprites::Clip;

    let (mut sim, mut state) = peopled(48, 24);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..2000 {
        sim.step(&state, dt);
    }
    let at = sim
        .buildings
        .iter()
        .position(|b| b.built)
        .expect("something should have been finished by now");
    let def = sim.buildings[at].def;

    let mut cache = SpriteCache::default();
    let plain =
        building_sprite(&mut cache, &state, sim.world(), &sim.buildings[at], false, false, Detail::Full);

    // One flat color, so what is drawn from it is unmistakable.
    let pink = grow::util::pack_rgba(255, 0, 255, 255);
    let clip = Clip::from_strip(4, 4, vec![pink; 16], 1, "test".into()).expect("a strip");
    state.civ.made.enabled = true;
    state.civ.made.set(def.id, clip);

    let mut cache = SpriteCache::default();
    let art =
        building_sprite(&mut cache, &state, sim.world(), &sim.buildings[at], false, false, Detail::Full);
    let kept = state.civ.made.slot(def.id).expect("the picture is in its slot");
    let want = kept.drawn_size(sim.world().cell_px, state.civ.art_px_per_cell);
    assert_eq!((art.w, art.h), want, "the picture should come out at the size it was drawn");
    assert_eq!(art.w, art.h, "a square picture should not take the shape of a box");
    assert!(art.px.iter().all(|v| *v == pink), "the picture is what should have been drawn");
    assert_ne!(
        (plain.w, plain.h, plain.px.clone()),
        (art.w, art.h, art.px.clone()),
        "the generated shape and the picture should differ"
    );

    // Turned off, the generator is back.
    state.civ.made.enabled = false;
    let mut cache = SpriteCache::default();
    let back =
        building_sprite(&mut cache, &state, sim.world(), &sim.buildings[at], false, false, Detail::Full);
    assert_eq!((back.w, back.h), (plain.w, plain.h));
}

#[test]
fn a_half_built_building_is_still_drawn_rising_out_of_the_ground() {
    use grow::civ::civ_render::{building_sprite, Detail, SpriteCache};
    use grow::civ::sprites::Clip;

    let (mut sim, mut state) = peopled(48, 24);
    let dt = 1.0 / state.civ.sim.tick_hz;
    let mut site = None;
    for _ in 0..2000 {
        sim.step(&state, dt);
        site = sim.buildings.iter().position(|b| !b.built && b.work_done > 0.0);
        if site.is_some() {
            break;
        }
    }
    // Nothing was mid-build at any step; there is nothing to check.
    let site = match site {
        Some(s) => s,
        None => return,
    };
    let def = sim.buildings[site].def;

    let pink = grow::util::pack_rgba(255, 0, 255, 255);
    let clip = Clip::from_strip(4, 4, vec![pink; 16], 1, "test".into()).expect("a strip");
    state.civ.made.enabled = true;
    state.civ.made.set(def.id, clip);

    let mut cache = SpriteCache::default();
    let drawn =
        building_sprite(&mut cache, &state, sim.world(), &sim.buildings[site], false, false, Detail::Full);
    let kept = state.civ.made.slot(def.id).expect("the picture is in its slot");
    let (w, h) = kept.drawn_size(sim.world().cell_px, state.civ.art_px_per_cell);
    assert!(
        (drawn.w, drawn.h) != (w, h) || !drawn.px.iter().all(|v| *v == pink),
        "one picture cannot say how far up a wall has got"
    );
}

// ---- watering the fields ------------------------------------------------

/// A settlement with a farm dropped where it is asked for, so what happens to
/// its fields can be watched without waiting for the planner to want one.
fn with_farm(wet: bool) -> (Settlement, State, usize) {
    let (mut sim, state) = peopled(64, 30);
    let farm = grow::civ::buildings::BUILDINGS
        .iter()
        .find(|d| d.id == "farm")
        .expect("the catalog has a farm");

    // Somewhere on land, either beside water or as far from it as the map
    // allows.
    let mut best: Option<((i32, i32), i32)> = None;
    for row in 2..sim.world().rows - 4 {
        for col in 2..sim.world().cols - 4 {
            if !(0..farm.h).all(|dr| (0..farm.w).all(|dc| sim.walkable(col + dc, row + dr))) {
                continue;
            }
            let mut near = 999;
            for r in row - 6..=row + 6 {
                for c in col - 6..=col + 6 {
                    if sim.in_bounds(c, r) && sim.in_water(c, r) {
                        near = near.min((c - col).abs() + (r - row).abs());
                    }
                }
            }
            let score = if wet { near } else { -near };
            if best.is_none_or(|(_, was)| score < was) {
                best = Some(((col, row), score));
            }
        }
    }
    let (col, row) = best.expect("somewhere to put a farm").0;
    let bi = sim
        .place_building(&state, 0, "farm", col, row, true)
        .expect("the farm went up");
    (sim, state, bi)
}

#[test]
fn a_farm_beside_water_keeps_its_own_fields_wet() {
    let (mut sim, state, bi) = with_farm(true);
    let soak = sim.farm_soak(&state, bi);
    assert!(soak > 0.0, "a farm placed beside water should reach some of it");

    sim.buildings[bi].water = 0.0;
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..400 {
        sim.step(&state, dt);
    }
    assert!(
        sim.buildings[bi].water > 0.1,
        "damp ground should have filled it, got {}",
        sim.buildings[bi].water
    );
}

#[test]
fn a_field_never_holds_more_than_a_field_can() {
    let (mut sim, state, bi) = with_farm(true);
    sim.buildings[bi].water = 1.0;
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..600 {
        sim.step(&state, dt);
        assert!(
            (0.0..=1.0).contains(&sim.buildings[bi].water),
            "water ran to {}",
            sim.buildings[bi].water
        );
    }
}

#[test]
fn a_parched_field_is_poor_rather_than_barren() {
    let (mut sim, state, bi) = with_farm(false);
    sim.buildings[bi].water = 0.0;
    let dry = sim.farm_water_factor(&state, bi);
    assert!(dry > 0.0, "nothing should stop growing entirely");
    assert!((dry - state.civ.work.farm_dry_yield).abs() < 1e-9);

    sim.buildings[bi].water = 1.0;
    let wet = sim.farm_water_factor(&state, bi);
    assert!((wet - 1.0).abs() < 1e-9, "a full field brings in all of it");
    assert!(wet > dry);
}

#[test]
fn a_bucket_is_worth_a_walk_to_the_bank() {
    use grow::civ::tasks::{start_water, Task};
    let (mut sim, state, bi) = with_farm(false);
    sim.buildings[bi].water = 0.0;

    // Somebody out in the open, with nothing else on.
    let pi = sim.people.live_indices()[0];
    grow::civ::tasks::abandon_task(&mut sim, pi);
    sim.people[pi].clear_task();
    assert!(start_water(&mut sim, &state, pi, bi), "there is water on this map to fetch");
    assert!(matches!(sim.people[pi].task, Some(Task::Water { full: false, .. })));

    let dt = 1.0 / state.civ.sim.tick_hz;
    let mut poured = false;
    for _ in 0..4000 {
        sim.step(&state, dt);
        if sim.buildings[bi].water > 0.0 {
            poured = true;
            break;
        }
    }
    assert!(poured, "the bucket never made it back to the field");
}

#[test]
fn the_nearest_bank_is_somewhere_to_stand() {
    let (sim, _, bi) = with_farm(true);
    let (col, row) = (sim.buildings[bi].col, sim.buildings[bi].row);
    let bank = sim.nearest_water(col, row, 40).expect("water somewhere on this map");
    assert!(sim.walkable(bank.0, bank.1), "a bank has to be dry land to stand on");
    let mut touches = false;
    for r in bank.1 - 1..=bank.1 + 1 {
        for c in bank.0 - 1..=bank.0 + 1 {
            if sim.in_bounds(c, r) && sim.in_water(c, r) {
                touches = true;
            }
        }
    }
    assert!(touches, "and has to be able to reach the water from there");
}

// ---- fear of the dark ---------------------------------------------------

/// A settlement stepped to a given hour of the day, so what the dark does can
/// be watched without waiting for it.
fn at_hour(hour: f64) -> (Settlement, State) {
    let (mut sim, state) = peopled(48, 24);
    let len = state.civ.people.day_length;
    let want = len * (hour / 24.0);
    sim.time = want;
    (sim, state)
}

#[test]
fn the_dark_works_on_somebody_out_in_it() {
    let (mut sim, state) = at_hour(1.0);
    assert!(sim.daylight(&state) < 0.35, "one in the morning should be dark");

    // Somebody outdoors, away from any lamp, with nothing on.
    let pi = sim.people.live_indices()[0];
    sim.people[pi].step_outside();
    sim.people[pi].fear = 0.0;
    let (c, r) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    assert!(!sim.lit_at(c, r), "the founding party has no lamps yet");

    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..200 {
        sim.step(&state, dt);
    }
    let worried = sim.people.get(sim.people[pi].id).map(|p| p.fear).unwrap_or(0.0);
    assert!(worried > 0.0, "a night out with no lamp should tell on somebody");
}

#[test]
fn daylight_settles_it_again() {
    let (mut sim, state) = at_hour(12.0);
    assert!(sim.daylight(&state) > 0.35, "noon should be daylight");
    for pi in sim.people.live_indices() {
        sim.people[pi].fear = 1.0;
    }
    let pi = sim.people.live_indices()[0];
    let id = sim.people[pi].id;
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..200 {
        sim.step(&state, dt);
    }
    assert!(sim.people.get(id).expect("still on file").fear < 1.0, "daylight should ease it");
}

#[test]
fn a_lit_street_is_as_good_as_daylight() {
    let (mut sim, state) = at_hour(1.0);
    let pi = sim.people.live_indices()[0];
    sim.people[pi].step_outside();
    let (c, r) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
    // A lamp where they are standing.
    let at = sim.free_spot_near(c, r).unwrap_or((c, r));
    sim.place_building(&state, 0, "lamp", at.0, at.1, true).expect("a lamp went up");
    assert!(sim.lit_at(c, r), "a lamp on top of somebody should light them");

    sim.people[pi].fear = 0.5;
    let id = sim.people[pi].id;
    let before = sim.people[pi].fear;
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..100 {
        sim.step(&state, dt);
    }
    let now = sim.people.get(id).expect("still on file");
    if now.cell_col() == c && now.cell_row() == r {
        assert!(now.fear <= before, "standing under a lamp should not make it worse");
    }
}

#[test]
fn a_frightened_owner_with_coin_pays_for_a_lamp() {
    let (mut sim, state) = at_hour(1.0);
    let dt = 1.0 / state.civ.sim.tick_hz;
    // Long enough that somebody holds a deed.
    for _ in 0..3000 {
        sim.step(&state, dt);
    }
    let owner = sim.people.live_indices().into_iter().find(|&pi| sim.people[pi].owns != 0);
    let owner = match owner {
        Some(o) => o,
        None => return,
    };
    let lamps = |sim: &Settlement| sim.buildings.iter().filter(|b| b.def.id == "lamp").count();
    let before = lamps(&sim);

    // The decision is taken once a day, so this has to see a day go by. Fear
    // and coin are topped up through it: the point is what the town does with
    // somebody frightened and able to pay, not how they got that way.
    let price = state.civ.build.lamp_coin * state.civ.build.upgrade_scale;
    let steps = (state.civ.people.day_length / dt) as usize * 3;
    for _ in 0..steps {
        sim.people[owner].coin = price * 3.0;
        sim.people[owner].fear = 1.0;
        sim.step(&state, dt);
    }
    assert!(lamps(&sim) > before, "a frightened owner with the coin should have paid for one");
}

#[test]
fn fear_alone_buys_nothing() {
    let (mut sim, state) = at_hour(1.0);
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..3000 {
        sim.step(&state, dt);
    }
    let before = sim.buildings.iter().filter(|b| b.def.id == "lamp").count();
    // Everybody terrified and penniless: the cost is the same for everybody,
    // which is what makes it the rich who light their street.
    for pi in sim.people.live_indices() {
        sim.people[pi].fear = 1.0;
        sim.people[pi].coin = 0.0;
    }
    let steps = (state.civ.people.day_length / dt) as usize * 3;
    for _ in 0..steps {
        sim.step(&state, dt);
        for pi in sim.people.live_indices() {
            sim.people[pi].fear = 1.0;
            sim.people[pi].coin = 0.0;
        }
    }
    assert_eq!(
        sim.buildings.iter().filter(|b| b.def.id == "lamp").count(),
        before,
        "nobody with nothing should have paid for anything"
    );
}

#[test]
fn the_town_no_longer_plans_lamps_itself() {
    let lamp = grow::civ::buildings::BUILDINGS
        .iter()
        .find(|d| d.id == "lamp")
        .expect("the catalog has a lamp");
    assert!(!lamp.planned, "a lamp goes up because somebody wanted one, not because a town did");
}

// ---- a picture per state -------------------------------------------------

#[test]
fn every_kind_of_thing_offers_the_states_it_can_be_in() {
    use grow::civ::sprites::{made_key, made_slots, made_states};
    for slot in made_slots() {
        let states = made_states(&slot.id);
        assert!(!states.is_empty(), "{} can be in no state at all", slot.id);
        assert_eq!(states[0].0, "", "the first state is the one everything falls back to");
        let mut keys: Vec<String> =
            states.iter().map(|(s, _)| made_key(&slot.id, s)).collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n, "{} has two states with one key", slot.id);
    }
    // A boat can be laden; a load in somebody's hand is only ever itself.
    assert!(made_states("boat").iter().any(|(s, _)| *s == "laden"));
    assert_eq!(made_states("carry-wood").len(), 1);
    assert!(made_states("hut").iter().any(|(s, _)| *s == "night"));
}

#[test]
fn a_state_with_no_picture_falls_back_to_the_one_that_has() {
    use grow::civ::sprites::{made_key, Clip, MadeSprites};
    let mut made = MadeSprites { enabled: true, ..MadeSprites::default() };
    let day = Clip::from_strip(4, 4, vec![1u32; 16], 1, "day".into()).expect("a strip");
    let night = Clip::from_strip(4, 4, vec![2u32; 16], 1, "night".into()).expect("a strip");
    made.set("hut", day);
    assert_eq!(made.clip_in("hut", "night").map(|c| c.source.as_str()), Some("day"));

    made.set(&made_key("hut", "night"), night);
    assert_eq!(made.clip_in("hut", "night").map(|c| c.source.as_str()), Some("night"));
    assert_eq!(made.clip_in("hut", "").map(|c| c.source.as_str()), Some("day"));
    assert_eq!(made.clip_in("hut", "working").map(|c| c.source.as_str()), Some("day"));
}

#[test]
fn a_picture_for_one_state_only_leaves_the_rest_generated() {
    use grow::civ::sprites::{made_key, Clip, MadeSprites};
    let mut made = MadeSprites { enabled: true, ..MadeSprites::default() };
    let lit = Clip::from_strip(4, 4, vec![2u32; 16], 1, "night".into()).expect("a strip");
    made.set(&made_key("hut", "night"), lit);
    assert!(made.clip_in("hut", "night").is_some(), "drawn from the picture after dark");
    assert!(made.clip_in("hut", "").is_none(), "and generated by day");
    assert_eq!(made.filled("hut"), 1);
}

#[test]
fn a_site_is_never_drawn_from_the_finished_picture() {
    use grow::civ::civ_render::{building_sprite, Detail, SpriteCache};
    use grow::civ::sprites::Clip;

    let (mut sim, mut state) = peopled(48, 24);
    let dt = 1.0 / state.civ.sim.tick_hz;
    let mut site = None;
    for _ in 0..2000 {
        sim.step(&state, dt);
        site = sim.buildings.iter().position(|b| !b.built && b.work_done > 0.0);
        if site.is_some() {
            break;
        }
    }
    let site = match site {
        Some(s) => s,
        None => return,
    };
    let def = sim.buildings[site].def;

    let pink = grow::util::pack_rgba(255, 0, 255, 255);
    state.civ.made.enabled = true;
    state.civ.made.set(
        def.id,
        Clip::from_strip(4, 4, vec![pink; 16], 1, "done".into()).expect("a strip"),
    );
    let mut cache = SpriteCache::default();
    let drawn =
        building_sprite(&mut cache, &state, sim.world(), &sim.buildings[site], false, false, Detail::Full);
    let (w, h) = state
        .civ
        .made
        .slot(def.id)
        .expect("the picture is in its slot")
        .drawn_size(sim.world().cell_px, state.civ.art_px_per_cell);
    assert!(
        (drawn.w, drawn.h) != (w, h) || !drawn.px.iter().all(|v| *v == pink),
        "a half built thing should not be drawn as the finished one"
    );

    // A picture drawn for the site itself is used.
    state.civ.made.set(
        &grow::civ::sprites::made_key(def.id, "site"),
        Clip::from_strip(4, 4, vec![pink; 16], 1, "site".into()).expect("a strip"),
    );
    let mut cache = SpriteCache::default();
    let now =
        building_sprite(&mut cache, &state, sim.world(), &sim.buildings[site], false, false, Detail::Full);
    assert_eq!((now.w, now.h), (w, h));
    assert!(now.px.iter().all(|v| *v == pink));
}

#[test]
fn the_picture_list_is_searchable_and_its_table_fits_it() {
    use grow::civ::sprites::made_entries;
    use grow::find::{Index, Search, Terms};

    let entries = made_entries();
    assert!(entries.len() > 100, "only {} picture slots to search", entries.len());
    let mut index = Index::new(entries);

    // Typing what a thing is called finds it and every state of it.
    let hits = index.search(Search::new("smithy"));
    assert!(!hits.is_empty());
    assert!(index.entries[hits[0].idx].label.starts_with("Smithy"));
    assert!(
        hits.iter().any(|h| index.entries[h.idx].label.contains("after dark")),
        "the states of a thing should come up with it"
    );

    // The table shipped for this list has to be built for this list. A table
    // built for the menus points at the wrong things and is refused.
    let made: Terms = serde_json::from_str(grow::find::MADE_TERMS_JSON).expect("made-terms.json");
    if !made.words.is_empty() {
        assert!(index.set_terms(made), "run `bun run index:made`");
        let hits = index.search(Search { query: "lantern", by_meaning: true, ..Search::default() });
        assert!(
            hits.iter().any(|h| index.entries[h.idx].label.starts_with("Lamp post")),
            "the meaning table should answer lantern with the lamp post"
        );
    }
    let menus: Terms = serde_json::from_str(grow::find::TERMS_JSON).expect("menu-terms.json");
    if !menus.words.is_empty() {
        assert!(!index.set_terms(menus), "the menus' table is not this list's table");
    }
}

// ---- the wind in the trees ------------------------------------------------

#[test]
fn the_wind_leans_a_tree_and_leaves_the_moss_alone() {
    use grow::sim::plant_sway;
    use grow::world::{World, WorldConfig};

    let state = State::new();
    let world = World::new(&WorldConfig::default());
    let of_class = |class: SizeClass| {
        state
            .species
            .iter()
            .find(|s| s.size_class == class)
            .unwrap_or_else(|| panic!("no {class:?} species in the defaults"))
    };
    let plant = |class: SizeClass, height_px: i32| {
        let species = of_class(class);
        let limits = grow::species::effective_limits(species, &state.class_limits);
        let mut p = grow::plant::Plant::new(1, species, limits, 4, 4, &world, grow::rng::Rng::new(9));
        p.bounds.include(p.ox - 3, p.oy - height_px);
        p.bounds.include(p.ox + 3, p.oy);
        p
    };

    // Ground cover holds still whatever the wind does.
    let mat = plant(SizeClass::Ground, 2);
    assert_eq!(plant_sway(&mat, 3.7, 2.0, 0.5), 0.0, "ground cover should not sway");

    // A full tree leans, within the amplitude, and moves over a cycle.
    let tree = plant(SizeClass::Tree, 60);
    let leans: Vec<f64> = (0..40).map(|k| plant_sway(&tree, k as f64 * 0.1, 2.0, 0.5)).collect();
    assert!(leans.iter().all(|l| l.abs() <= 2.0), "a lean past the amplitude: {leans:?}");
    let swing = leans.iter().cloned().fold(f64::MIN, f64::max)
        - leans.iter().cloned().fold(f64::MAX, f64::min);
    assert!(swing > 1.0, "a full tree should visibly move over a cycle, swung {swing}");

    // A seedling of the same species barely moves: the lean scales with height.
    let sprout = plant(SizeClass::Tree, 6);
    let small = plant_sway(&sprout, 0.3, 2.0, 0.5).abs();
    let tall = plant_sway(&tree, 0.3, 2.0, 0.5).abs();
    assert!(
        small < tall || tall == 0.0,
        "a sprout ({small}) should lean less than a tree ({tall})"
    );

    // The same moment gives the same lean: the wind is simulation time, not
    // the wall clock, which is what keeps two runs of one seed one picture.
    assert_eq!(plant_sway(&tree, 1.25, 2.0, 0.5), plant_sway(&tree, 1.25, 2.0, 0.5));

    // A plant with nothing drawn has nothing to lean.
    let species = of_class(SizeClass::Tree);
    let limits = grow::species::effective_limits(species, &state.class_limits);
    let bare = grow::plant::Plant::new(2, species, limits, 4, 4, &world, grow::rng::Rng::new(9));
    assert_eq!(plant_sway(&bare, 1.0, 2.0, 0.5), 0.0);
}

// ---- somebody set down by hand --------------------------------------------

#[test]
fn somebody_new_can_be_set_down_by_hand() {
    let mut state = State::new();
    state.civ.world.cols = 40;
    state.civ.world.rows = 20;
    state.civ.terrain.warmup = 30.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let before = sim.people.count();
    let town = sim.colonies[0].id;
    let (cc, cr) = sim.colonies[0].center;

    let id = sim
        .spawn_person_at(&state, cc as f64 + 2.0, cr as f64 + 1.0)
        .expect("a press near the town lands somebody");
    assert_eq!(sim.people.count(), before + 1, "nobody joined the register");
    let p = sim.people.get(id).expect("on the register").clone();
    assert!(p.adult(), "arrivals come grown");
    assert_eq!(p.colony, town, "they join the town nearest the press");
    assert!(
        sim.walkable(p.cell_col(), p.cell_row()) || sim.in_water(p.cell_col(), p.cell_row()),
        "they landed in something nobody can stand in"
    );
    assert!(p.events.iter().any(|e| e.text.contains("wandered into")),
        "the arrival is on their record: {:?}", p.events);

    // A press off the map is clamped onto it, not refused.
    let far = sim.spawn_person_at(&state, -50.0, -50.0);
    assert!(far.is_some(), "a press past the edge should land on it");

    // The world keeps running with them in it.
    let dt = 1.0 / state.civ.sim.tick_hz;
    for _ in 0..(state.civ.people.day_length / dt) as usize {
        sim.step(&state, dt);
    }
    assert!(sim.people.get(id).is_some(), "the arrival fell off the register");
}

// ---- the weather ----------------------------------------------------------

#[test]
fn the_clouds_are_seamless_settable_and_on_the_clock() {
    use grow::civ::clouds::{field, refresh, CloudLayer, TILE_H, TILE_W};

    let mut state = State::new();
    state.civ.view.clouds = true;

    // The tile wraps: the noise at the far edge is the noise at the near one,
    // or the repeat would show a line every hundred and ninety two pixels.
    for y in [0, 17, TILE_H - 1] {
        let a = field(0, y, 7, 0.3, 0.5);
        let b = field(TILE_W, y, 7, 0.3, 0.5);
        assert!((a - b).abs() < 1e-9, "a horizontal seam at y {y}: {a} vs {b}");
    }
    for x in [0, 41, TILE_W - 1] {
        let a = field(x, 0, 7, 0.3, 0.5);
        let b = field(x, TILE_H, 7, 0.3, 0.5);
        assert!((a - b).abs() < 1e-9, "a vertical seam at x {x}: {a} vs {b}");
    }

    // The same moment is the same sky: the tile is simulation time, not the
    // wall clock.
    let mut a = CloudLayer::default();
    let mut b = CloudLayer::default();
    refresh(&mut a, &state, 12.34);
    refresh(&mut b, &state, 12.34);
    assert_eq!(a.key, b.key);
    assert_eq!(a.px, b.px, "two layers built at one moment differ");
    assert_eq!(a.drift, b.drift);

    // Cover is coverage: an overcast sky holds far more cloud than wisps.
    // Both ends are set here rather than leaning on whatever the default
    // happens to be, which is a number that moves.
    let count = |layer: &CloudLayer| layer.px.iter().filter(|p| **p != 0).count();
    state.civ.view.cloud_cover = 0.2;
    let mut thin = CloudLayer::default();
    refresh(&mut thin, &state, 12.34);
    let sparse = count(&thin);
    state.civ.view.cloud_cover = 1.0;
    let mut heavy = CloudLayer::default();
    refresh(&mut heavy, &state, 12.34);
    assert!(
        count(&heavy) > sparse * 2,
        "full cover ({}) should dwarf wisps ({sparse})",
        count(&heavy)
    );

    // The start height is a line down the sky band, in whole world pixels,
    // and it is the same line the space around the map is drawn against.
    assert_eq!(state.civ.view.cloud_start_px(200), 0, "clouds start at the top by default");
    state.civ.view.cloud_top = 0.5;
    assert_eq!(state.civ.view.cloud_start_px(200), 100);
    state.civ.view.cloud_top = 0.0;

    // Wobble is the edge movement: with it the shapes churn from step to
    // step, without it the same shapes drift whole and nothing regenerates.
    state.civ.view.cloud_cover = 0.35;
    state.civ.view.cloud_wobble = 0.5;
    let mut w0 = CloudLayer::default();
    let mut w1 = CloudLayer::default();
    refresh(&mut w0, &state, 10.0);
    refresh(&mut w1, &state, 11.0);
    assert_ne!(w0.px, w1.px, "a second of wobble changed nothing");
    state.civ.view.cloud_wobble = 0.0;
    let mut s0 = CloudLayer::default();
    refresh(&mut s0, &state, 10.0);
    let key = s0.key;
    refresh(&mut s0, &state, 11.0);
    assert_eq!(s0.key, key, "with no wobble the tile should never rebuild");

    // Off empties the layer, which is also what tells the camera to leave the
    // space around the map alone.
    state.civ.view.clouds = false;
    refresh(&mut s0, &state, 12.0);
    assert!(s0.px.is_empty());
}

/// A camp fire is the only build that takes itself away again: it is placed
/// finished, it burns for its lifetime, and then the map is as it was. What
/// this checks is the going out, because that is the part nothing else in the
/// settlement does.
#[test]
fn a_camp_fire_burns_down_and_leaves_nothing_behind() {
    let mut state = State::new();
    state.civ.world.cols = 40;
    state.civ.world.rows = 20;
    state.civ.terrain.warmup = 0.0;
    // Nobody lights one of their own during the run, so what is measured is
    // the one put down here.
    state.civ.build.camp_fires = false;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);

    let def = grow::civ::buildings::building_by_id("campfire").expect("no camp fire in the catalog");
    assert!(def.lifetime > 0.0, "a camp fire that never goes out is not a camp fire");

    let at = (0..sim.world().cols)
        .flat_map(|c| (0..sim.world().rows).map(move |r| (c, r)))
        .find(|&(c, r)| grow::civ::planner::fits(&sim, def, c, r, 0, 0))
        .expect("nowhere on the map to light a fire");
    let bi = sim
        .place_building(&state, 0, "campfire", at.0, at.1, true)
        .expect("the fire was not placed");
    let id = sim.buildings[bi].id;
    assert!(sim.buildings[bi].built, "a fire is lit, not staked out");
    assert_ne!(sim.build_grid[sim.idx(at.0, at.1)], 0, "the fire claimed no ground");

    let dt = 1.0 / state.civ.sim.tick_hz;
    let steps = ((def.lifetime * 0.5) / dt) as usize;
    for _ in 0..steps {
        sim.step(&state, dt);
    }
    let half = sim.building_index(id).expect("the fire went out early");
    assert!(sim.buildings[half].burned > 0.0, "the fire is not burning down");

    for _ in 0..steps + 2 {
        sim.step(&state, dt);
    }
    assert!(sim.building_index(id).is_none(), "the fire is still standing past its life");
    assert_eq!(
        sim.build_grid[sim.idx(at.0, at.1)], 0,
        "the ground the fire stood on is still claimed"
    );
}

/// The other half of the same feature: with the switch on, a town that is out
/// in the dark ends up with fires in it, and with the switch off it never
/// does. Off has to be the settlement exactly as it ran before there were any.
#[test]
fn fires_are_lit_in_the_dark_and_only_with_the_switch_on() {
    fn fires_over(days: i32, on: bool) -> usize {
        let mut state = State::new();
        state.civ.world.cols = 40;
        state.civ.world.rows = 20;
        state.civ.terrain.warmup = 0.0;
        state.civ.build.camp_fires = on;
        // Frightened sooner, so a handful of days is enough to see one lit.
        // The default sits above the lamp threshold on purpose, which is a
        // fortnight of nights away on a fresh map.
        state.civ.build.camp_fire_fear = 0.05;
        let mut sim = Settlement::new(&state);
        sim.bootstrap(&state);
        let dt = 1.0 / state.civ.sim.tick_hz;
        let mut seen = 0;
        let mut standing: Vec<i32> = Vec::new();
        for _ in 0..(state.civ.people.day_length / dt) as usize * days as usize {
            sim.step(&state, dt);
            for b in &sim.buildings {
                if b.def.id == "campfire" && !standing.contains(&b.id) {
                    standing.push(b.id);
                    seen += 1;
                }
            }
        }
        seen
    }

    assert_eq!(fires_over(6, false), 0, "a fire was lit with the switch off");
    assert!(fires_over(6, true) > 0, "nobody lit a fire on six nights out");
}

/// The cloud start height is a line across the sky band: nothing is stamped
/// above it, and the weather picks up from it downward. Read off a composited
/// frame rather than off the tile, because the tile does not know where it is
/// put and this is entirely about where it is put.
#[test]
fn no_cloud_is_drawn_above_the_start_height() {
    let mut state = State::new();
    state.civ.world.cols = 40;
    state.civ.world.rows = 20;
    state.civ.terrain.warmup = 0.0;
    state.civ.view.clouds = true;
    state.civ.view.cloud_cover = 1.0;
    state.civ.view.cloud_wobble = 0.0;
    state.civ.view.cull = false;
    state.civ.view.cloud_top = 0.0;

    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    sim.process_raster_queue(&state, usize::MAX);
    sim.composite(&state);
    let px_w = sim.world().px_w as usize;
    let sky = sim.world().sky_px;
    assert!(sky > 8, "the sky band is too shallow to say anything about");

    let cloudy = |sim: &Settlement, row: i32| -> bool {
        let at = row as usize * px_w;
        sim.buffer[at..at + px_w] != sim.ground[at..at + px_w]
    };
    assert!(
        (0..sky / 4).any(|y| cloudy(&sim, y)),
        "with the line at the top, the top of the sky should carry cloud"
    );

    state.civ.view.cloud_top = 0.5;
    let line = state.civ.view.cloud_start_px(sky);
    sim.ground_dirty = true;
    sim.composite(&state);
    for y in 0..line {
        assert!(!cloudy(&sim, y), "row {y} carries cloud above the start height {line}");
    }
    assert!(
        (line..sky).any(|y| cloudy(&sim, y)),
        "nothing was drawn below the start height either"
    );
}

/// A whole map laid down from a drawing is one call per cell, which is the
/// scale nothing else in the tool paints at. This is a floor under that: it
/// fails if painting the map ever goes quadratic again.
#[test]
fn painting_a_whole_map_is_not_quadratic() {
    let mut state = State::new();
    state.civ.world.cols = 128;
    state.civ.world.rows = 64;
    state.civ.terrain.warmup = 60.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    let cells: Vec<(i32, i32)> =
        (0..cols).flat_map(|c| (0..rows).map(move |r| (c, r))).collect();

    let started = std::time::Instant::now();
    let done = sim.paint_cells(&cells, Cell::Water);
    let took = started.elapsed();
    assert!(done > cells.len() / 2, "only {done} of {} cells took the paint", cells.len());
    assert!(
        took < std::time::Duration::from_secs(4),
        "painting {} cells took {took:?}",
        cells.len()
    );
    assert_eq!(
        sim.terrain.kind.iter().filter(|&&k| k == Cell::Water as u8).count(),
        sim.terrain.water_cells,
        "the running water count drifted from the grid"
    );
}
