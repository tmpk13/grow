// Headless check of the settlement (no DOM): founds a settlement, runs it for
// a stretch of days, verifies the bookkeeping and writes a PPM snapshot.
//
//   bun run tools/civsmoke.js [days] [outfile.ppm]

import { writeFileSync } from 'node:fs';
import { createState } from '../src/state.js';
import { Settlement } from '../src/civ/settlement.js';
import { RES_IDS } from '../src/civ/resources.js';
import { CELL } from '../src/civ/terrain.js';
import { unpackRGBA } from '../src/util.js';

const days = Number(process.argv[2] || 60);
const out = process.argv[3] || 'settlement.ppm';

const state = createState();
const sim = new Settlement(state);
sim.bootstrap();

const dt = 1 / state.civ.sim.tickHz;
const stepsPerDay = Math.round(state.civ.people.dayLength / dt);
let errors = 0;

const fail = (msg) => {
  console.error(`  ${msg}`);
  errors++;
};

console.log(`${sim.name}: ${sim.people.length} people, ${sim.plantSim.plants.length} plants`);
console.log('day  pop  built  tech  food  wood  plank  brick  metal  tool  coin  piles');
for (let day = 0; day < days; day++) {
  for (let i = 0; i < stepsPerDay; i++) {
    sim.step(dt);
    sim.processRasterQueue(24);
  }
  if (day % 10 === 9 || day === days - 1) {
    const s = sim.stats();
    console.log(
      [
        String(s.day).padStart(3),
        String(s.population).padStart(4),
        String(s.buildings).padStart(6),
        String(s.known).padStart(5),
        String(Math.round(sim.stock.food)).padStart(5),
        String(Math.round(sim.stock.wood)).padStart(5),
        String(Math.round(sim.stock.plank)).padStart(6),
        String(Math.round(sim.stock.brick)).padStart(6),
        String(Math.round(sim.stock.metal)).padStart(6),
        String(Math.round(sim.stock.tool)).padStart(5),
        String(Math.round(s.coin)).padStart(5),
        String(sim.piles.length).padStart(6),
      ].join(' '),
    );
  }
}

// Grid bookkeeping: every claimed cell belongs to a building that says it is
// there, and nothing was built on water.
const byId = new Map(sim.buildings.map((b) => [b.id, b]));
for (let row = 0; row < sim.world.rows; row++) {
  for (let col = 0; col < sim.world.cols; col++) {
    const id = sim.buildGrid[sim.idx(col, row)];
    if (!id) continue;
    const b = byId.get(id);
    if (!b) {
      fail(`cell ${col},${row} claimed by missing building ${id}`);
      continue;
    }
    if (col < b.col || col >= b.col + b.w || row < b.row || row >= b.row + b.h) {
      fail(`building ${b.type} ${b.id} claims a cell outside its footprint`);
    }
    if (sim.terrain.type[sim.terrain.idx(col, row)] === CELL.water) {
      fail(`building ${b.type} ${b.id} stands on water`);
    }
    if (!sim.blocked[sim.idx(col, row)]) fail(`cell ${col},${row} is built on but not blocked for plants`);
  }
}
for (const b of sim.buildings) {
  for (let row = b.row; row < b.row + b.h; row++) {
    for (let col = b.col; col < b.col + b.w; col++) {
      if (sim.buildGrid[sim.idx(col, row)] !== b.id) fail(`building ${b.id} does not own ${col},${row}`);
    }
  }
  for (const id of b.workers) {
    const p = sim.people.find((q) => q.id === id);
    if (!p) fail(`building ${b.type} ${b.id} lists a worker who is not alive`);
    else if (p.work !== b.id) fail(`worker ${p.name} does not agree they work at ${b.type}`);
  }
}

// No plants where a building stands, and no plant claimed by a ghost.
for (const plant of sim.plantSim.plants) {
  if (sim.blocked[sim.idx(plant.col, plant.row)]) fail(`plant ${plant.id} grows on blocked ground`);
  if (plant.claimedBy && !sim.people.some((p) => p.id === plant.claimedBy)) {
    fail(`plant ${plant.id} is claimed by a person who is gone`);
  }
}

// Books: nothing negative, nothing reserved that is not there.
for (const id of RES_IDS) {
  if ((sim.stock[id] || 0) < -0.001) fail(`negative stock of ${id}`);
  if ((sim.stockReserved[id] || 0) > (sim.stock[id] || 0) + 0.001) {
    fail(`more ${id} reserved (${sim.stockReserved[id].toFixed(1)}) than in store (${(sim.stock[id] || 0).toFixed(1)})`);
  }
}
for (const pile of sim.piles) {
  if (pile.n <= 0) fail('a pile with nothing in it is still on the map');
  if (pile.claimedBy && !sim.people.some((p) => p.id === pile.claimedBy)) {
    fail('a pile is claimed by a person who is gone');
  }
}
for (const p of sim.people) {
  if (p.x < 0 || p.x > sim.world.cols || p.y < 0 || p.y > sim.world.rows) fail(`${p.name} walked off the map`);
  if (p.work && !byId.has(p.work)) fail(`${p.name} works at a building that is gone`);
  if (p.home && !byId.has(p.home)) fail(`${p.name} lives in a building that is gone`);
  if (p.carry.n > 0 && !p.carry.res) fail(`${p.name} carries nothing in particular`);
}

const stats = sim.stats();
console.log(`\n${sim.name} on day ${stats.day}`);
console.log(`  people ${stats.population} (${stats.children} children), beds ${stats.housing}, happiness ${stats.happiness.toFixed(2)}`);
console.log(`  born ${stats.births}, died ${stats.deaths}`);
console.log(`  buildings ${stats.buildings} built, ${stats.sites} under construction`);
console.log(`  technologies ${stats.known}/${stats.techs}: ${sim.tech.known.join(', ') || 'none'}`);
const jobs = Object.entries(stats.professions).map(([k, v]) => `${k} ${v}`).join(', ');
console.log(`  work: ${jobs}`);

sim.composite();
const w = sim.world.pxW;
const h = sim.world.pxH;
const header = `P6\n${w} ${h}\n255\n`;
const bytes = new Uint8Array(header.length + w * h * 3);
for (let i = 0; i < header.length; i++) bytes[i] = header.charCodeAt(i);
for (let i = 0; i < w * h; i++) {
  const c = unpackRGBA(sim.buffer[i]);
  bytes[header.length + i * 3] = c.r;
  bytes[header.length + i * 3 + 1] = c.g;
  bytes[header.length + i * 3 + 2] = c.b;
}
writeFileSync(out, bytes);
console.log(`\nwrote ${out} (${w}x${h})`);

if (errors) {
  console.error(`${errors} problems`);
  process.exit(1);
}
console.log('bookkeeping consistent');
