//! The simulation has to stay reproducible: one seed, one run, one picture.
//! These are the invariants the headless smoke binaries check on a long run,
//! kept small enough to run on every `cargo test`.

use grow::civ::civ_render::Detail;
use grow::civ::settlement::{Rect, Settlement};
use grow::civ::sprites::{motion_of, natural_cmp, Clip, Motion, PeopleSprites, MAX_FRAME_PX};
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
        married += sim.people.iter().filter(|p| p.spouse != 0).count();
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
        "only {married} married adults across {} towns",
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
