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
        "only {married} settlers ever married across {} towns",
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

// ---- settler sprites -----------------------------------------------------

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

#[test]
fn a_settler_reports_the_motion_they_are_in() {
    let mut sim = Settlement::new(&State::new());
    sim.bootstrap(&State::new());
    let state = State::new();
    let dt = 1.0 / state.civ.sim.tick_hz;
    // A whole day of a working town should show every settler in a motion that
    // matches what the record says they are doing.
    let mut swam = false;
    for _ in 0..(state.civ.people.day_length * state.civ.sim.tick_hz) as usize {
        sim.step(&state, dt);
        let seen: Vec<(bool, grow::civ::sprites::Motion)> = sim
            .people
            .iter_indexed()
            .map(|(_, p)| {
                let wet = sim.in_water(p.cell_col(), p.cell_row());
                (wet, motion_of(p, wet))
            })
            .collect();
        for ((wet, motion), (_, p)) in seen.iter().zip(sim.people.iter_indexed()) {
            swam |= *wet;
            match motion {
                Motion::Sleep => assert!(p.sleeping),
                Motion::Swim => assert!(*wet && !p.sleeping),
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
        let p = sim.people.get(id).expect("a settler went missing");
        assert_eq!((p.x, p.y), (x, y), "a settler moved when the map grew");
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
    state.civ.sprites.walk.as_mut().unwrap().height = 1.75;
    let back = State::from_json(&state.to_json()).expect("reload");
    let clip = back.civ.sprites.walk.as_ref().expect("the walk survived");
    assert_eq!(clip.frame_count(), 4);
    assert_eq!(clip.frame_w(), 6);
    assert_eq!(clip.h, 8);
    assert_eq!(clip.height, 1.75);
    for f in 0..4 {
        assert_eq!(clip.pixel(f, 0, 0), (f + 1) as u32, "frame {f} came back wrong");
    }
}

#[test]
fn a_settler_behind_a_bush_is_drawn_behind_it() {
    // Draw order is depth, not the row something stands in. Two things in one
    // row used to tie and then be separated by what kind of thing they were,
    // which put every settler in front of every plant in their row - so
    // somebody walking behind a bush walked over it.
    use grow::civ::civ_render::depth_key;

    // A plant stands in the middle of its cell; a settler stands wherever they
    // are in theirs.
    let bush = depth_key(4.0 + 0.5);
    assert!(depth_key(4.2) < bush, "a settler at the back of the row is not behind the bush");
    assert!(depth_key(4.8) > bush, "a settler at the front of the row is not in front of it");
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
    // No beds and no coin, so every settler takes the last resort.
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
        "{far_asleep} of {asleep} sleeping moments were far from town: settlers are \
         still bedding down where the day ended"
    );
}

/// A small town, warmed up enough to have settlers walking about in it.
fn peopled(cols: i32, rows: i32) -> (Settlement, State) {
    let mut state = State::new();
    state.civ.world.cols = cols;
    state.civ.world.rows = rows;
    state.civ.terrain.warmup = 60.0;
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    (sim, state)
}

/// The first settler out in the open, who is the one a press on their own feet
/// has to find.
fn outdoors(sim: &Settlement) -> u32 {
    sim.people
        .iter()
        .find(|p| !p.indoors() && p.aboard == 0)
        .map(|p| p.id)
        .expect("nobody is outdoors to pick up")
}

#[test]
fn a_settler_is_picked_up_by_pointing_at_them() {
    let (sim, _) = peopled(48, 24);
    let id = outdoors(&sim);
    let (x, y) = {
        let p = sim.people.get(id).unwrap();
        (p.x, p.y)
    };
    assert_eq!(sim.person_near(x, y, 1.6), Some(id), "pointing at somebody's feet missed them");
    // A settler is drawn standing up out of their cell, so the reach is taller
    // above them than below.
    assert_eq!(sim.person_near(x, y - 2.0, 1.6), Some(id), "pointing at somebody's head missed them");
    // Far enough away and the answer is nobody rather than the nearest.
    let far = sim.person_near(x + 40.0, y, 1.6);
    assert!(far.is_none() || far != Some(id), "a press nowhere near anybody picked somebody up");
}

#[test]
fn a_settler_can_be_carried_somewhere_else_and_put_down() {
    let (mut sim, _) = peopled(48, 24);
    let id = outdoors(&sim);
    let was = {
        let p = sim.people.get(id).unwrap();
        (p.cell_col(), p.cell_row())
    };
    assert!(sim.hold_person(id), "a living settler could not be picked up");
    assert_eq!(sim.held, id);

    // Somewhere they can stand, as far from where they were as the map allows.
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    let target = (0..rows)
        .flat_map(|r| (0..cols).map(move |c| (c, r)))
        .filter(|(c, r)| sim.walkable(*c, *r))
        .max_by_key(|(c, r)| (c - was.0).abs() + (r - was.1).abs())
        .expect("the map has nowhere to stand");
    sim.move_held(target.0 as f64 + 0.5, target.1 as f64 + 0.5);
    assert_eq!(sim.drop_held(), Some(target), "a settler was not put down where they were let go");
    assert_eq!(sim.held, 0, "the hand is still holding somebody");

    let p = sim.people.get(id).unwrap();
    assert_ne!((p.cell_col(), p.cell_row()), was, "the settler did not move at all");
    assert!(p.task.is_none(), "a settler put down is still on their way somewhere");
    assert!(p.path.is_empty(), "a settler put down is still walking a path from before");
    assert!(!p.indoors(), "a settler picked up is still recorded as being inside");
}

#[test]
fn a_settler_put_down_on_a_roof_lands_beside_it() {
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
    assert_ne!(landed, blocked, "a settler was left standing in something solid");
    assert!(sim.walkable(landed.0, landed.1), "a settler landed somewhere they cannot stand");
}

#[test]
fn a_settler_in_hand_is_left_out_of_the_tick() {
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
    assert_eq!((p.x, p.y), held_at, "a settler being held walked off on their own");
    assert!(p.task.is_none(), "a settler being held took on work");
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
        "a settler put down never picked their life back up"
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

// ---- foliage over a settler ---------------------------------------------

/// One settler pixel and one plant pixel in a two pixel buffer, so what
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
fn solid_foliage_covers_a_settler_the_way_a_plant_does() {
    use grow::util::is_person;
    let leaf = grow::util::pack_rgba(0, 255, 0, 255);
    let out = over_person(grow::sim::Foliage::Solid, leaf);
    assert_eq!(out, [leaf, leaf]);
    assert!(!is_person(out[0]), "solid foliage is not the settler any more");
}

#[test]
fn hatched_foliage_leaves_every_other_pixel_showing() {
    use grow::util::{is_person, mark_person, unpack_rgba};
    let leaf = grow::util::pack_rgba(0, 255, 0, 255);
    let out = over_person(grow::sim::Foliage::Hatched, leaf);
    let kept = unpack_rgba(out[0]);
    assert_eq!((kept.r, kept.g, kept.b), (255, 0, 0), "one pixel stays the settler");
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
    assert!(is_person(out[0]), "the settler is still under it, so the next leaf fades too");
}

#[test]
fn the_settler_mark_rides_in_the_alpha_and_changes_nothing_visible() {
    use grow::util::{is_person, mark_person, unpack_rgba, PERSON_ALPHA};
    let color = grow::util::pack_rgba(120, 40, 200, 255);
    let marked = mark_person(color);
    assert!(is_person(marked));
    assert!(!is_person(color), "an ordinary opaque pixel is not a settler");
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
fn a_picture_fills_the_box_the_generator_would_have() {
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
    use grow::civ::civ_render::{building_sprite, made_box, Detail, SpriteCache};
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
    let (w, h) = made_box(sim.world(), def);
    assert_eq!((art.w, art.h), (w, h), "the picture should fill the box it was given");
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
    use grow::civ::civ_render::{building_sprite, made_box, Detail, SpriteCache};
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
    let (w, h) = made_box(sim.world(), def);
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
    use grow::civ::civ_render::{building_sprite, made_box, Detail, SpriteCache};
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
    let (w, h) = made_box(sim.world(), def);
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
