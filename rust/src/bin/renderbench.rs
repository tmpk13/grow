//! Times the settlement drawing, headless, one phase at a time.
//!
//!   cargo run --release --bin renderbench -- [days]
//!
//! The browser measures whole frames (`tools/perfbench.js`); this measures the
//! two halves that are plain Rust, so a change can be checked without a
//! browser in the loop. The first table is the compositing at each sampling
//! step the camera can ask for, and the second is each detail level with the
//! periodic ground rebuild and the repack half of the upload broken out.
//!
//! GROW_COLS, GROW_ROWS and GROW_SEED work as in civsmoke. GROW_EXTRA plants a
//! given number of extra plants before drawing, for the crowded end of the
//! range: growing that many takes far longer than drawing them.

use std::time::Instant;

use grow::civ::civ_render::{composite_settlement, Detail};
use grow::civ::settlement::{Rect, Settlement};
use grow::state::State;

fn env_i32(key: &str, fallback: i32) -> i32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(fallback)
}

/// Milliseconds per composite, averaged over a short run.
fn time_composites(sim: &mut Settlement, state: &State, n: u32) -> f64 {
    composite_settlement(sim, state);
    let t = Instant::now();
    for _ in 0..n {
        composite_settlement(sim, state);
    }
    t.elapsed().as_secs_f64() * 1000.0 / n as f64
}

fn main() {
    let days: i32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(60);
    let mut state = State::new();
    state.civ.world.cols = env_i32("GROW_COLS", 384);
    state.civ.world.rows = env_i32("GROW_ROWS", 192);
    state.civ.seed = env_i32("GROW_SEED", state.civ.seed as i32) as u32;

    let mut sim = Settlement::new(&state);
    sim.bootstrap(&state);
    let dt = 1.0 / state.civ.sim.tick_hz;
    let steps_per_day = (state.civ.people.day_length / dt).round() as i32;
    let t0 = Instant::now();
    for _ in 0..days * steps_per_day {
        sim.step(&state, dt);
    }
    println!(
        "{} days in {:.1}s: {} plants, {} buildings, {} people, {}x{} px",
        days,
        t0.elapsed().as_secs_f64(),
        sim.plant_sim.plants.len(),
        sim.buildings.len(),
        sim.people.count(),
        sim.world().px_w,
        sim.world().px_h
    );

    let extra = env_i32("GROW_EXTRA", 0);
    if extra > 0 {
        let (cols, rows) = (sim.world().cols, sim.world().rows);
        for i in 0..extra {
            let species = (i as usize) % state.species.len();
            let (col, row) = ((i * 7919) % cols, (i * 104_729) % rows);
            sim.plant_sim.try_spawn(&state, species, col, row, None);
        }
        while !sim.plant_sim.raster_queue.is_empty() {
            sim.plant_sim.process_raster_queue(&state, 512);
        }
        println!("planted up to {} plants", sim.plant_sim.plants.len());
    }

    let whole = Rect::whole(sim.world());
    sim.view = whole;

    // What thinning the ground restore is worth, which is most of a frame that
    // has the whole map in view.
    sim.detail = Detail::Blocks;
    for step in [1, 2, 4, 8] {
        sim.px_step = step;
        let ms = time_composites(&mut sim, &state, 20);
        println!("blocks, px_step {step}: composite {ms:6.2} ms");
    }

    sim.px_step = 1;
    let px_w = sim.world().px_w;
    let mut scratch = vec![0u32; (whole.x1 * whole.y1) as usize];
    for detail in [Detail::Full, Detail::Reduced, Detail::Coarse, Detail::Blocks] {
        sim.detail = detail;
        sim.ground_dirty = true;
        let per = time_composites(&mut sim, &state, 20);
        // The ground is rebuilt on a timer rather than every frame, so it is
        // timed as the difference one forced rebuild makes.
        let t = Instant::now();
        sim.ground_dirty = true;
        composite_settlement(&mut sim, &state);
        let rebuild = t.elapsed().as_secs_f64() * 1000.0 - per;
        // The repack half of present_region, which the browser cannot be asked
        // for separately.
        let n = 20;
        let t = Instant::now();
        for _ in 0..n {
            for y in 0..whole.y1 {
                let w = whole.x1 as usize;
                let (src, dst) = ((y * px_w) as usize, (y * whole.x1) as usize);
                scratch[dst..dst + w].copy_from_slice(&sim.buffer[src..src + w]);
            }
        }
        let repack = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!(
            "{:8}: composite {:6.2} ms   ground rebuild {:6.2} ms   repack {:5.2} ms",
            detail.label(),
            per,
            rebuild,
            repack
        );
    }
}
