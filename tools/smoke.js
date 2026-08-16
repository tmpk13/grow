// Headless check of the simulation core (no DOM): runs a world, verifies the
// grid occupancy rules and writes a PPM snapshot for eyeballing.
//
//   bun run tools/smoke.js [outfile.ppm]

import { writeFileSync } from 'node:fs';
import { createState } from '../src/state.js';
import { Sim, makePreviewPlant } from '../src/sim.js';
import { SIZE_CLASSES } from '../src/species.js';
import { unpackRGBA } from '../src/util.js';

const out = process.argv[2] || 'world.ppm';
const state = createState();
const sim = new Sim(state);

const stepDt = 1 / state.sim.tickHz;
for (let i = 0; i < 4000; i++) {
  sim.step(stepDt);
  sim.processRasterQueue(64);
}
sim.processRasterQueue(10000);
sim.composite();

const stats = sim.stats();
console.log(`plants: ${stats.total}, sim time: ${stats.time.toFixed(1)}`);
for (const sp of state.species) {
  console.log(`  ${sp.name.padEnd(18)} ${stats.perSpecies.get(sp.id) || 0}`);
}

// Occupancy invariants: one owner per cell per layer, and the owner must be a
// live plant of the matching size class.
let errors = 0;
const byId = new Map(sim.plants.map((p) => [p.id, p]));
for (let layer = 0; layer < sim.world.layers.length; layer++) {
  const grid = sim.world.layers[layer];
  for (let i = 0; i < grid.length; i++) {
    const owner = grid[i];
    if (owner === 0) continue;
    const plant = byId.get(owner);
    if (!plant) {
      console.error(`stale claim: layer ${layer} cell ${i} owned by missing plant ${owner}`);
      errors++;
      continue;
    }
    if (SIZE_CLASSES[plant.species.sizeClass].layer !== layer) {
      console.error(`layer mismatch: plant ${owner} in layer ${layer}`);
      errors++;
    }
  }
}

// Cells shared across layers prove that several items can occupy one cell.
let shared = 0;
for (let cy = 0; cy < sim.world.rows; cy++) {
  for (let cx = 0; cx < sim.world.cols; cx++) {
    const mask = sim.world.occupancyAt(cx, cy);
    if (mask && (mask & (mask - 1)) !== 0) shared++;
  }
}
console.log(`cells with more than one size class present: ${shared}`);

let painted = 0;
for (const p of sim.plants) {
  if (p.bounds.x1 >= p.bounds.x0) painted++;
}
console.log(`plants with rasterized pixels: ${painted}/${sim.plants.length}`);

const preview = makePreviewPlant(state, state.species.find((s) => s.id === 'sp-oak'), 99);
let guard = 0;
while (!preview.mature && guard < 5000) {
  preview.grow(1, preview.previewCtx);
  guard++;
}
preview.raster(sim.env);
console.log(
  `preview tree: segments ${preview.segments.length}, leaves ${preview.leaves.length}, ` +
    `bounds ${preview.bounds.x0},${preview.bounds.y0} to ${preview.bounds.x1},${preview.bounds.y1}`,
);

writePPM(out, sim.buffer, sim.world.pxW, sim.world.pxH);
console.log(`wrote ${out} (${sim.world.pxW}x${sim.world.pxH})`);
process.exit(errors ? 1 : 0);

function writePPM(path, buf, w, h) {
  const header = Buffer.from(`P6\n${w} ${h}\n255\n`, 'ascii');
  const body = Buffer.alloc(w * h * 3);
  for (let i = 0; i < w * h; i++) {
    const c = unpackRGBA(buf[i]);
    body[i * 3] = c.r;
    body[i * 3 + 1] = c.g;
    body[i * 3 + 2] = c.b;
  }
  writeFileSync(path, Buffer.concat([header, body]));
}
