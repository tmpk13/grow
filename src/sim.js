// Simulation: spawning, growth scheduling, grid bookkeeping and compositing
// the world pixel buffer.

import { World } from './world.js';
import { Plant, MAT, MAT_SLOT } from './plant.js';
import { SIZE_CLASSES, effectiveLimits } from './species.js';
import { findSampler, rampPick, samplerRamp } from './sampler.js';
import { makeRng } from './rng.js';
import { clamp, hash2, hexToPacked, mixPacked, packRGBA } from './util.js';

const SUPPORT_LAYERS = [SIZE_CLASSES.shrub.layer, SIZE_CLASSES.tree.layer];
const SHADOW_COLOR = packRGBA(6, 10, 14, 255);

// Ramps are looked up per pixel during shading, so they are resolved once per
// species and cached until the sampling boxes change.
export function makeEnv(state) {
  let cacheVersion = -1;
  let cache = new Map();
  return {
    get shading() {
      return state.shading;
    },
    rampsFor(species) {
      if (cacheVersion !== state.materials.version) {
        cacheVersion = state.materials.version;
        cache = new Map();
      }
      let ramps = cache.get(species.id);
      if (!ramps) {
        ramps = {};
        for (const [matId, slot] of Object.entries(MAT_SLOT)) {
          const sampler = findSampler(state.materials, species.slots[slot]);
          ramps[matId] = samplerRamp(state.materials, sampler);
        }
        cache.set(species.id, ramps);
      }
      return ramps;
    },
    invalidate() {
      cacheVersion = -1;
    },
  };
}

export class Sim {
  // The world config is passed separately from the state so a second sim (the
  // settlement map) can run the same species on a grid of its own size.
  constructor(state, worldCfg = state.world) {
    this.state = state;
    this.worldCfg = worldCfg;
    this.world = new World(worldCfg);
    this.env = makeEnv(state);
    // Cells plants may not take: set by the settlement for water and for the
    // ground its buildings stand on. Null means the whole grid is open.
    this.blocked = null;
    // How lush this world is: scales both how often a species seeds and how
    // many instances of it the world carries. The settlement map runs richer
    // than the lab because it is larger and because people eat what grows.
    this.wildScale = 1;
    this.reset(state.seed);
  }

  reset(seed = this.state.seed) {
    this.world.configure(this.worldCfg);
    this.world.clear();
    this.rng = makeRng(seed);
    this.plants = [];
    this.nextId = 1;
    this.time = 0;
    this.ticks = 0;
    this.buffer = new Uint32Array(this.world.pxW * this.world.pxH);
    this.bufferDirty = true;
    this.rasterQueue = [];
    this.env.invalidate();
  }

  resizeWorld() {
    this.reset(this.state.seed);
  }

  get ctx() {
    return {
      world: this.world,
      supportLayers: SUPPORT_LAYERS,
      requestSpace: (plant, r, h) => this.requestSpace(plant, r, h),
    };
  }

  step(dt) {
    this.time += dt;
    this.ticks++;
    this.spawnPhase(dt);
    const ctx = this.ctx;
    for (let i = this.plants.length - 1; i >= 0; i--) {
      const plant = this.plants[i];
      plant.grow(dt, ctx);
      if (!plant.alive) this.removePlantAt(i);
      else if (plant.dirty && this.rasterQueue.indexOf(plant) === -1) this.rasterQueue.push(plant);
    }
  }

  spawnPhase(dt) {
    const { species, classLimits } = this.state;
    for (const sp of species) {
      if (!sp.enabled) continue;
      const limits = effectiveLimits(sp, classLimits);
      const mine = this.plants.filter((p) => p.species.id === sp.id);
      const scale = this.wildScale || 1;
      if (mine.length >= limits.maxInstances * scale) continue;

      let attempts = sp.spawn.rate * scale * dt;
      while (attempts > 0) {
        if (attempts >= 1 || this.rng.chance(attempts)) {
          this.trySpawn(sp, this.rng.int(0, this.world.cols - 1), this.rng.int(0, this.world.rows - 1));
        }
        attempts -= 1;
      }

      // Offspring land somewhere on the ring around the parent, anywhere in
      // the area rather than only left or right of it.
      for (const parent of mine) {
        if (!this.rng.chance(sp.spread.rate * scale * dt)) continue;
        const dist = this.rng.range(sp.spread.radiusMin, sp.spread.radiusMax);
        const a = this.rng.range(0, Math.PI * 2);
        this.trySpawn(
          sp,
          Math.round(parent.col + Math.cos(a) * dist),
          Math.round(parent.row + Math.sin(a) * dist),
        );
      }
    }
  }

  trySpawn(sp, col, row) {
    const c = clamp(col | 0, 0, this.world.cols - 1);
    const r = clamp(row | 0, 0, this.world.rows - 1);
    if (this.blocked && this.blocked[r * this.world.cols + c]) return null;
    const limits = effectiveLimits(sp, this.state.classLimits);
    const layer = SIZE_CLASSES[sp.sizeClass].layer;
    if (this.world.hasNeighborWithin(layer, c, r, limits.minSpacing)) return null;

    const plant = new Plant({
      id: this.nextId,
      species: sp,
      limits,
      col: c,
      row: r,
      world: this.world,
      rng: makeRng(this.rng.seed()),
    });
    plant.layer = layer;
    plant.depthShade = this.depthShadeFor(r);
    const cells = this.world.footprint(c, r, 0);
    if (!this.world.canClaim(layer, cells, plant.id)) return null;
    this.world.claim(layer, cells, plant.id);
    plant.cells = cells.slice();
    plant.grantedRadiusCells = 0;
    this.nextId++;
    this.plants.push(plant);
    this.rasterQueue.push(plant);
    return plant;
  }

  // Distance haze: plants at the back of the area shade one step lighter,
  // which stays inside their own ramp instead of tinting them out of palette.
  depthShadeFor(row) {
    const far = this.world.rows > 1 ? 1 - row / (this.world.rows - 1) : 0;
    return far * (this.worldCfg.depthFade || 0);
  }

  requestSpace(plant, radiusCells) {
    const cells = this.world.footprint(plant.col, plant.row, radiusCells);
    if (!this.world.canClaim(plant.layer, cells, plant.id)) return false;
    if (this.blocked) {
      for (let i = 0; i < cells.length; i++) {
        if (this.blocked[cells[i]]) return false;
      }
    }
    this.world.release(plant.layer, plant.cells, plant.id);
    this.world.claim(plant.layer, cells, plant.id);
    plant.cells = cells.slice();
    return true;
  }

  removePlantAt(index) {
    const plant = this.plants[index];
    this.world.release(plant.layer, plant.cells, plant.id);
    this.plants.splice(index, 1);
    const q = this.rasterQueue.indexOf(plant);
    if (q !== -1) this.rasterQueue.splice(q, 1);
    this.bufferDirty = true;
  }

  removeAll() {
    this.world.clear();
    this.plants = [];
    this.rasterQueue = [];
    this.bufferDirty = true;
  }

  // Re-rasterizing every growing plant every frame is the expensive part, so
  // only a fixed number of plants are redrawn per frame; the rest catch up on
  // later frames.
  processRasterQueue(budget) {
    let n = 0;
    while (this.rasterQueue.length && n < budget) {
      const plant = this.rasterQueue.shift();
      if (!plant.alive) continue;
      plant.raster(this.env);
      this.bufferDirty = true;
      n++;
    }
    return n;
  }

  markAllDirty() {
    this.rasterQueue = this.plants.slice();
    for (const p of this.plants) p.dirty = true;
    this.bufferDirty = true;
  }

  // Painter's algorithm over the area: back rows first, and within a row the
  // flat items before the standing ones, so nearer plants overlap farther ones.
  composite() {
    const buf = this.buffer;
    this.paintBackground(buf);
    const shadows = this.worldCfg.shadows !== false;
    for (const plant of this.drawOrder()) this.blitPlant(buf, plant, shadows);
    this.bufferDirty = false;
    return buf;
  }

  // One plant onto a buffer with the world's dimensions, contact shadow first.
  blitPlant(buf, plant, shadows = true) {
    const w = this.world;
    const b = plant.bounds;
    if (b.x1 < b.x0) return;
    const anchorX = w.anchorX(plant.col);
    const anchorY = w.anchorY(plant.row);
    if (shadows && plant.species.sizeClass !== 'ground' && plant.radiusPx > 1) {
      this.castShadow(buf, anchorX, anchorY, plant);
    }
    const dx = anchorX - plant.ox;
    const dy = anchorY - plant.oy;
    for (let y = b.y0; y <= b.y1; y++) {
      const wy = y + dy;
      if (wy < 0 || wy >= w.pxH) continue;
      const srow = y * plant.w;
      const drow = wy * w.pxW;
      for (let x = b.x0; x <= b.x1; x++) {
        const v = plant.sprite[srow + x];
        if (v === 0) continue;
        const wx = x + dx;
        if (wx < 0 || wx >= w.pxW) continue;
        buf[drow + wx] = v;
      }
    }
  }

  // Back to front order for one frame: rows first, then flat items before
  // standing ones. The settlement merges its own drawables into this.
  drawOrder() {
    return [...this.plants].sort((a, b) => {
      if (a.row !== b.row) return a.row - b.row;
      const ao = SIZE_CLASSES[a.species.sizeClass].order;
      const bo = SIZE_CLASSES[b.species.sizeClass].order;
      if (ao !== bo) return ao - bo;
      return a.id - b.id;
    });
  }

  // Contact shadow: a foreshortened ellipse under the plant, dithered at the
  // rim so it stays pixel art rather than a soft blob.
  castShadow(buf, cx, cy, plant) {
    const w = this.world;
    const rx = Math.max(2, plant.radiusPx * 0.85);
    const ry = Math.max(1, rx * w.depthRatio);
    const x0 = Math.max(0, Math.floor(cx - rx));
    const x1 = Math.min(w.pxW - 1, Math.ceil(cx + rx));
    const y0 = Math.max(0, Math.floor(cy - ry));
    const y1 = Math.min(w.pxH - 1, Math.ceil(cy + ry));
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        const dx = (x + 0.5 - cx) / rx;
        const dy = (y + 0.5 - cy) / ry;
        const d = dx * dx + dy * dy;
        if (d > 1) continue;
        if (d > 0.45 && hash2(x, y, plant.seed) < (d - 0.45) / 0.55) continue;
        const i = y * w.pxW + x;
        buf[i] = mixPacked(buf[i], SHADOW_COLOR, 0.42);
      }
    }
  }

  paintBackground(buf) {
    const w = this.world;
    const cfg = this.worldCfg;
    const skyTop = hexToPacked(cfg.skyTop);
    const skyBottom = hexToPacked(cfg.skyBottom);
    for (let y = 0; y < w.skyPx; y++) {
      const t = w.skyPx > 1 ? y / (w.skyPx - 1) : 0;
      const c = mixPacked(skyTop, skyBottom, t);
      buf.fill(c, y * w.pxW, (y + 1) * w.pxW);
    }
    // The ground plane is dithered out of the soil ramp rather than tiled, so
    // the sampler art does not show up as stripes, and lifted toward the light
    // end of the ramp with distance so the far rows read as further away.
    const sampler = findSampler(this.state.materials, cfg.soilSampler);
    const ramp = sampler ? samplerRamp(this.state.materials, sampler) : [];
    const fallback = packRGBA(52, 38, 28, 255);
    const fade = cfg.depthFade || 0;
    for (let y = w.skyPx; y < w.pxH; y++) {
      const row = Math.min(w.rows - 1, Math.floor((y - w.skyPx) / w.depthPx));
      const far = w.rows > 1 ? 1 - row / (w.rows - 1) : 0;
      for (let x = 0; x < w.pxW; x++) {
        let c = fallback;
        if (ramp.length) {
          const noise = (hash2(x, y, 7331) - 0.5) * 0.24;
          const t = clamp(0.4 + far * fade * 2 + noise, 0, 1);
          c = rampPick(ramp, t);
        }
        buf[y * w.pxW + x] = c;
      }
    }
  }

  stats() {
    const perSpecies = new Map();
    for (const p of this.plants) {
      perSpecies.set(p.species.id, (perSpecies.get(p.species.id) || 0) + 1);
    }
    return { total: this.plants.length, perSpecies, time: this.time, ticks: this.ticks };
  }
}

// Isolated single plant for the species preview: no neighbors, no grid
// contention, so the form parameters can be judged on their own.
export function makePreviewPlant(state, species, seed) {
  const limits = effectiveLimits(species, state.classLimits);
  const cellPx = state.world.cellPx;
  const fakeWorld = {
    cellPx,
    depthRatio: (state.world.depthPx || cellPx) / cellPx,
    findSupport: () => null,
  };
  const plant = new Plant({
    id: 1,
    species,
    limits,
    col: 0,
    row: 0,
    world: fakeWorld,
    rng: makeRng(seed),
  });
  plant.layer = SIZE_CLASSES[species.sizeClass].layer;
  // No requestSpace in this context, so the plant grows to its own limits.
  plant.previewCtx = { world: fakeWorld, supportLayers: [] };
  return plant;
}

export { MAT };
