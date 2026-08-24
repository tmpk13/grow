//! Headless check of the simulation core (no browser): runs a world, verifies
//! the grid occupancy rules and writes a PPM snapshot for eyeballing.
//!
//!   cargo run --release --bin smoke -- [outfile.ppm]

use std::fs::File;
use std::io::{BufWriter, Write};

use grow::plant::Scratch;
use grow::sim::{Preview, Sim};
use grow::species::SIZE_CLASSES;
use grow::state::State;
use grow::util::unpack_rgba;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "world.ppm".to_string());
    let state = State::new();
    let mut sim = Sim::new(&state, state.world.clone());

    let step_dt = 1.0 / state.sim.tick_hz;
    for _ in 0..4000 {
        sim.step(&state, step_dt, None);
        sim.process_raster_queue(&state, 64);
    }
    sim.process_raster_queue(&state, usize::MAX);
    sim.composite(&state);

    let stats = sim.stats();
    println!("plants: {}, sim time: {:.1}", stats.total, stats.time);
    for sp in &state.species {
        println!(
            "  {:<18} {}",
            sp.name,
            stats.per_species.get(&sp.id).copied().unwrap_or(0)
        );
    }

    // Occupancy invariants: one owner per cell per layer, and the owner must be
    // a live plant of the matching size class.
    let mut errors = 0;
    for layer in 0..sim.world.layers.len() {
        for i in 0..sim.world.layers[layer].len() {
            let owner = sim.world.layers[layer][i];
            if owner == 0 {
                continue;
            }
            match sim.plants.iter().find(|p| p.id == owner) {
                None => {
                    eprintln!("stale claim: layer {layer} cell {i} owned by missing plant {owner}");
                    errors += 1;
                }
                Some(plant) => {
                    if plant.size_class.layer() != layer {
                        eprintln!("layer mismatch: plant {owner} in layer {layer}");
                        errors += 1;
                    }
                }
            }
        }
    }

    // Cells shared across layers prove that several items can occupy one cell.
    let mut shared = 0;
    for cy in 0..sim.world.rows {
        for cx in 0..sim.world.cols {
            let mask = sim.world.occupancy_at(cx, cy);
            if mask != 0 && (mask & (mask - 1)) != 0 {
                shared += 1;
            }
        }
    }
    println!("cells with more than one size class present: {shared}");
    println!("size classes: {}", SIZE_CLASSES.len());

    let painted = sim.plants.iter().filter(|p| !p.bounds.is_empty()).count();
    println!("plants with rasterized pixels: {painted}/{}", sim.plants.len());

    let species = state.find_species("sp-oak").expect("oak species");
    let mut preview = Preview::new(&state, species, 99);
    let mut guard = 0;
    while !preview.plant.mature() && guard < 5000 {
        preview.grow(1.0, species);
        guard += 1;
    }
    let mut env = grow::sim::Env::default();
    let mut scratch = Scratch::default();
    preview.raster(&state, &mut env, &mut scratch, species);
    let b = preview.plant.bounds;
    println!(
        "preview tree: segments {}, leaves {}, bounds {},{} to {},{}",
        preview.plant.segments.len(),
        preview.plant.leaves.len(),
        b.x0,
        b.y0,
        b.x1,
        b.y1
    );

    write_ppm(&out, &sim.buffer, sim.world.px_w, sim.world.px_h);
    println!("wrote {out} ({}x{})", sim.world.px_w, sim.world.px_h);
    std::process::exit(if errors > 0 { 1 } else { 0 });
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
