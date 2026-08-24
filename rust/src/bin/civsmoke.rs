//! Headless check of the settlement (no browser): founds a settlement, runs it
//! for a stretch of days, verifies the bookkeeping and writes a PPM snapshot.
//!
//!   cargo run --release --bin civsmoke -- [days] [outfile.ppm] [detail]
//!
//! `detail` is full, reduced, coarse or blocks, and exercises the same drawing
//! path the camera picks when it is zoomed out.

use std::fs::File;
use std::io::{BufWriter, Write};

use grow::civ::buildings::Structure;
use grow::civ::civ_render::Detail;
use grow::civ::resources::{Res, RES_IDS};
use grow::civ::settlement::Settlement;
use grow::civ::terrain::Cell;
use grow::state::State;
use grow::util::unpack_rgba;

fn main() {
    let days: i32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(60);
    let out = std::env::args().nth(2).unwrap_or_else(|| "settlement.ppm".to_string());
    let detail = match std::env::args().nth(3).unwrap_or_default().as_str() {
        "reduced" => Detail::Reduced,
        "coarse" => Detail::Coarse,
        "blocks" => Detail::Blocks,
        _ => Detail::Full,
    };

    let mut state = State::new();
    // A larger map than the default, for checking that the big end still works:
    //   GROW_COLS=384 GROW_ROWS=192 cargo run --release --bin civsmoke -- 20
    if let Ok(v) = std::env::var("GROW_COLS") {
        if let Ok(n) = v.parse() {
            state.civ.world.cols = n;
        }
    }
    if let Ok(v) = std::env::var("GROW_ROWS") {
        if let Ok(n) = v.parse() {
            state.civ.world.rows = n;
        }
    }
    // One seed is one world, and a settlement is chaotic enough that a single
    // run says nothing about a change. This is what makes a sweep possible.
    if let Ok(v) = std::env::var("GROW_SEED") {
        if let Ok(n) = v.parse() {
            state.civ.seed = n;
        }
    }
    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);

    let dt = 1.0 / state.civ.sim.tick_hz;
    let steps_per_day = (state.civ.people.day_length / dt).round() as i32;
    let mut errors = 0;
    let fail = |msg: String, errors: &mut i32| {
        eprintln!("  {msg}");
        *errors += 1;
    };

    println!(
        "{}: {} settlers, {} plants, {} rivers",
        sim.name,
        sim.people.count(),
        sim.plant_sim.plants.len(),
        sim.terrain.rivers.len()
    );
    println!("day  pop  wed  towns  built  tech  food  wood  plank  brick  metal  tool  coin  boats  piles");
    for day in 0..days {
        for _ in 0..steps_per_day {
            sim.step(&state, dt);
            sim.process_raster_queue(&state, 24);
        }
        if day % 10 == 9 || day == days - 1 {
            let s = sim.stats(&state);
            let total = sim.total_stock();
            let g = |res: Res| total[res as usize].round() as i64;
            let wed = sim
                .people
                .iter()
                .filter(|p| p.spouse != 0 && p.adult())
                .count();
            println!(
                "{:>3} {:>4} {:>4} {:>6} {:>6} {:>5} {:>5} {:>5} {:>6} {:>6} {:>6} {:>5} {:>5} {:>6} {:>6}",
                s.day,
                s.population,
                wed,
                s.colonies.len(),
                s.buildings,
                s.known,
                g(Res::Food),
                g(Res::Wood),
                g(Res::Plank),
                g(Res::Brick),
                g(Res::Metal),
                g(Res::Tool),
                s.coin.round() as i64,
                s.boats,
                sim.piles.len()
            );
        }
    }

    // Grid bookkeeping: every claimed cell belongs to a building that says it
    // is there, and nothing was built on water.
    for row in 0..sim.world().rows {
        for col in 0..sim.world().cols {
            let id = sim.build_grid[sim.idx(col, row)];
            if id == 0 {
                continue;
            }
            let bi = match sim.building_index(id) {
                Some(bi) => bi,
                None => {
                    fail(format!("cell {col},{row} claimed by missing building {id}"), &mut errors);
                    continue;
                }
            };
            let b = &sim.buildings[bi];
            if !b.covers(col, row) {
                fail(
                    format!("building {} {} claims a cell outside its footprint", b.def.id, b.id),
                    &mut errors,
                );
            }
            if sim.terrain.kind[sim.terrain.idx(col, row)] == Cell::Water as u8 {
                fail(format!("building {} {} stands on water", b.def.id, b.id), &mut errors);
            }
            if sim.blocked[sim.idx(col, row)] == 0 {
                fail(
                    format!("cell {col},{row} is built on but not blocked for plants"),
                    &mut errors,
                );
            }
        }
    }
    for b in &sim.buildings {
        for row in b.row..b.row + b.h {
            for col in b.col..b.col + b.w {
                if sim.build_grid[sim.idx(col, row)] != b.id {
                    fail(format!("building {} does not own {col},{row}", b.id), &mut errors);
                }
            }
        }
        if sim.colony_index(b.colony).is_none() {
            fail(format!("building {} belongs to no colony", b.id), &mut errors);
        }
        for &id in &b.workers {
            match sim.people.get(id) {
                None => fail(
                    format!("building {} {} lists a worker who is not on file", b.def.id, b.id),
                    &mut errors,
                ),
                Some(p) if !p.alive => fail(
                    format!("building {} {} lists a worker who is dead", b.def.id, b.id),
                    &mut errors,
                ),
                Some(p) if p.work != b.id => fail(
                    format!("worker {} does not agree they work at {}", p.name, b.def.id),
                    &mut errors,
                ),
                _ => {}
            }
        }
        if b.owner != 0 {
            // A home is held by its deed, a stall by whoever stands behind it.
            // They are separate slots on the person for exactly this reason:
            // an owner keeps their house when they take on a counter.
            let stall = b.def.structure == Structure::Stall;
            match sim.people.get(b.owner) {
                Some(p) if stall && p.stall != b.id => fail(
                    format!("{} does not agree they keep stall {}", p.name, b.id),
                    &mut errors,
                ),
                Some(p) if !stall && p.owns != b.id => fail(
                    format!("{} does not agree they own building {}", p.name, b.id),
                    &mut errors,
                ),
                None => fail(format!("building {} is deeded to nobody on file", b.id), &mut errors),
                _ => {}
            }
        }
        // A gate is the one thing that claims a cell and still lets people
        // cross it, and only once it is actually standing.
        for row in b.row..b.row + b.h {
            for col in b.col..b.col + b.w {
                let open = sim.gates[sim.idx(col, row)] != 0;
                let want = b.built && b.def.structure == Structure::Gate;
                if open != want {
                    fail(
                        format!("{} {} at {col},{row} is {} but the gate grid says {}",
                            b.def.id, b.id,
                            if want { "a way through" } else { "shut" },
                            if open { "open" } else { "shut" }),
                        &mut errors,
                    );
                }
                if sim.walkable(col, row) != want {
                    fail(format!("{} {} at {col},{row} disagrees with walkable()", b.def.id, b.id), &mut errors);
                }
            }
        }
    }

    // Nobody is walled in. This is the one thing a ring can get wrong that
    // nothing else would notice: every settler has to still have a way to the
    // middle of their own town.
    for pi in sim.people.live_indices() {
        if sim.people[pi].aboard != 0 {
            continue;
        }
        let (c, r) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
        let ci = sim.colony_of(pi);
        let center = sim.colonies[ci].center;
        if sim.find_path(c, r, center.0, center.1).is_none() {
            let name = sim.people[pi].name.clone();
            let town = sim.colonies[ci].name.clone();
            fail(format!("{name} at {c},{r} cannot reach {town}"), &mut errors);
        }
    }

    // Bonds: one per person met, never to oneself, and always a real number.
    for p in sim.people.archive() {
        let mut seen: Vec<u32> = Vec::new();
        for bond in &p.bonds {
            if bond.who == p.id {
                fail(format!("{} keeps a bond with themself", p.name), &mut errors);
            }
            if seen.contains(&bond.who) {
                fail(format!("{} keeps two bonds with the same person", p.name), &mut errors);
            }
            seen.push(bond.who);
            if !bond.affinity.is_finite() || bond.affinity.abs() > 1.001 {
                fail(
                    format!("{} feels {} about somebody", p.name, bond.affinity),
                    &mut errors,
                );
            }
        }
        if p.met_count() > state.civ.social.memory.max(4) {
            fail(format!("{} remembers more people than they can", p.name), &mut errors);
        }
    }

    // No plants where a building stands, and no plant claimed by a ghost.
    for plant in &sim.plant_sim.plants {
        if sim.blocked[sim.idx(plant.col, plant.row)] != 0 {
            fail(format!("plant {} grows on blocked ground", plant.id), &mut errors);
        }
        if plant.claimed_by != 0 && !sim.people.is_alive(plant.claimed_by) {
            fail(
                format!("plant {} is claimed by a settler who is gone", plant.id),
                &mut errors,
            );
        }
    }

    // Books: nothing negative, nothing reserved that is not there.
    for (ci, colony) in sim.colonies.iter().enumerate() {
        for res in RES_IDS {
            let have = colony.stock[res as usize];
            let held = colony.stock_reserved[res as usize];
            if have < -0.001 {
                fail(format!("{} has negative {}", colony.name, res.id()), &mut errors);
            }
            if held > have + 0.001 {
                fail(
                    format!(
                        "{} has more {} reserved ({held:.1}) than in store ({have:.1})",
                        colony.name,
                        res.id()
                    ),
                    &mut errors,
                );
            }
        }
        let _ = ci;
    }
    for pile in &sim.piles {
        if pile.n <= 0.0 {
            fail("a pile with nothing in it is still on the map".into(), &mut errors);
        }
        if pile.claimed_by != 0 && !sim.people.is_alive(pile.claimed_by) {
            fail("a pile is claimed by a settler who is gone".into(), &mut errors);
        }
    }
    for p in sim.people.iter() {
        if p.x < 0.0 || p.x > sim.world().cols as f64 || p.y < 0.0 || p.y > sim.world().rows as f64 {
            fail(format!("{} walked off the map", p.name), &mut errors);
        }
        if p.work != 0 && sim.building_index(p.work).is_none() {
            fail(format!("{} works at a building that is gone", p.name), &mut errors);
        }
        if p.home != 0 && sim.building_index(p.home).is_none() {
            fail(format!("{} lives in a building that is gone", p.name), &mut errors);
        }
        if p.owns != 0 && sim.building_index(p.owns).is_none() {
            fail(format!("{} holds a deed to nothing", p.name), &mut errors);
        }
        match sim.building_index(p.stall) {
            _ if p.stall == 0 => {}
            Some(bi) if sim.buildings[bi].owner == p.id => {}
            Some(_) => fail(format!("{} keeps a stall that is not theirs", p.name), &mut errors),
            None => fail(format!("{} keeps a stall that is gone", p.name), &mut errors),
        }
        if p.carry.n > 0.0 && p.carry.res.is_none() {
            fail(format!("{} carries nothing in particular", p.name), &mut errors);
        }
        if sim.colony_index(p.colony).is_none() {
            fail(format!("{} belongs to no colony", p.name), &mut errors);
        }
    }
    for boat in &sim.boats {
        if sim.building_index(boat.home_dock).is_none() {
            fail(format!("{} is moored at a dock that is gone", boat.name), &mut errors);
        }
        let (c, r) = boat.cell();
        if !sim.terrain.navigable(c, r) {
            fail(format!("{} is aground at {c},{r}", boat.name), &mut errors);
        }
    }

    let stats = sim.stats(&state);
    println!("\n{} on day {}", sim.name, stats.day);
    println!(
        "  people {} ({} children), beds {}, happiness {:.2}",
        stats.population, stats.children, stats.housing, stats.happiness
    );
    println!("  born {}, died {}, on file {}", stats.births, stats.deaths, sim.people.slots());
    println!(
        "  buildings {} built, {} under construction",
        stats.buildings, stats.sites
    );
    for c in &stats.colonies {
        println!(
            "  {}: {} people, {} buildings, {} beds, {} coin in the treasury, {} in purses, {} techs",
            c.name,
            c.population,
            c.buildings,
            c.housing,
            c.coin.round() as i64,
            c.wealth.round() as i64,
            c.known
        );
    }
    let mut homes: Vec<(&str, usize, usize)> = Vec::new();
    for b in &sim.buildings {
        if b.def.housing == 0 || !b.built {
            continue;
        }
        match homes.iter_mut().find(|(id, _, _)| *id == b.def.id) {
            Some(e) => {
                e.1 += 1;
                e.2 += (b.owner != 0) as usize;
            }
            None => homes.push((b.def.id, 1, (b.owner != 0) as usize)),
        }
    }
    let homes: Vec<String> = homes
        .iter()
        .map(|(id, n, owned)| format!("{n} {id} ({owned} deeded)"))
        .collect();
    println!("  homes: {}", homes.join(", "));
    let inns = sim.buildings.iter().filter(|b| b.built && b.def.is_inn).count();
    let docks = sim.buildings.iter().filter(|b| b.built && b.def.is_dock).count();
    let indoors = sim.people.iter().filter(|p| p.indoors()).count();
    println!("  inns {inns}, docks {docks}, indoors right now {indoors}");

    let standing = |kind: Structure| {
        sim.buildings.iter().filter(|b| b.built && b.def.structure == kind).count()
    };
    let kept = sim.people.iter().filter(|p| p.stall != 0).count();
    let counters: f64 = sim
        .buildings
        .iter()
        .filter(|b| b.built && b.def.structure == Structure::Stall)
        .map(|b| b.inv.iter().sum::<f64>())
        .sum();
    println!(
        "  walls {}, gates {}, stalls {} ({kept} kept, {counters:.0} on the counters)",
        standing(Structure::Wall),
        standing(Structure::Gate),
        standing(Structure::Stall)
    );

    let mut bonds = 0usize;
    let mut friendships = 0usize;
    let mut feuds = 0usize;
    let mut fondest: Option<(f32, String, String)> = None;
    for p in sim.people.iter() {
        bonds += p.bonds.len();
        for bond in &p.bonds {
            if bond.affinity >= state.civ.social.friend_at as f32 {
                friendships += 1;
            }
            if bond.affinity <= -state.civ.social.friend_at as f32 {
                feuds += 1;
            }
            if fondest.as_ref().is_none_or(|(a, _, _)| bond.affinity > *a) {
                let other = sim
                    .people
                    .get(bond.who)
                    .map(|q| q.name.clone())
                    .unwrap_or_else(|| "somebody gone".into());
                fondest = Some((bond.affinity, p.name.clone(), other));
            }
        }
    }
    let heads = sim.people.count().max(1);
    println!(
        "  bonds {bonds} ({:.1} each), {friendships} friendships, {feuds} feuds",
        bonds as f64 / heads as f64
    );
    if let Some((a, one, other)) = fondest {
        println!("  closest: {one} and {other} ({a:.2})");
    }

    let mut open_sites: Vec<String> = Vec::new();
    for b in &sim.buildings {
        if b.built {
            continue;
        }
        let short: Vec<String> = b
            .cost
            .iter()
            .filter_map(|&(res, n)| {
                let gap = n - b.delivered[res as usize];
                if gap > 0.0 {
                    Some(format!("{} {}", gap.ceil(), res.id()))
                } else {
                    None
                }
            })
            .collect();
        open_sites.push(format!(
            "{} at {},{} {}",
            b.def.id,
            b.col,
            b.row,
            if short.is_empty() {
                format!("raising {:.0}%", b.work_done / b.work.max(1.0) * 100.0)
            } else {
                format!("waiting on {}", short.join(", "))
            }
        ));
    }
    if !open_sites.is_empty() {
        println!("  sites: {}", open_sites.join("; "));
    }
    if let Some(&pi) = sim.wealthiest(1).first() {
        let p = &sim.people[pi];
        println!(
            "  richest: {} the {}, {} coin, owns {}",
            p.name,
            p.profession.label().to_lowercase(),
            p.coin.round() as i64,
            sim.building_index(p.owns)
                .map(|bi| sim.buildings[bi].def.label)
                .unwrap_or("nothing")
        );
    }
    for river in &sim.terrain.rivers {
        println!(
            "  the {} runs {} cells{}",
            river.name,
            river.path.len(),
            if river.reaches_sea { "" } else { " and peters out" }
        );
    }
    println!("  boats {}", sim.boats.len());
    let jobs = stats
        .professions
        .iter()
        .map(|(p, n)| format!("{} {}", p.label().to_lowercase(), n))
        .collect::<Vec<_>>()
        .join(", ");
    println!("  work: {jobs}");

    sim.detail = detail;
    sim.composite(&state);
    let (w, h) = (sim.world().px_w, sim.world().px_h);
    write_ppm(&out, &sim.buffer, w, h);
    println!("\nwrote {out} ({w}x{h}) at {} detail", detail.label());

    if errors > 0 {
        eprintln!("{errors} problems");
        std::process::exit(1);
    }
    println!("bookkeeping consistent");
}

fn write_ppm(path: &str, buf: &[u32], w: i32, h: i32) {
    let file = File::create(path).expect("create ppm");
    let mut out = BufWriter::new(file);
    write!(out, "P6\n{w} {h}\n255\n").unwrap();
    let mut body = Vec::with_capacity((w * h * 3) as usize);
    for &v in buf {
        let c = unpack_rgba(v);
        body.push(c.r);
        body.push(c.g);
        body.push(c.b);
    }
    out.write_all(&body).unwrap();
}
