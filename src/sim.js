// Simulation: spawning, growth scheduling, grid bookkeeping and compositing
// the world pixel buffer.

import { World } from './world.js';
import { Plant, MAT, MAT_SLOT } from './plant.js';
import { SIZE_CLASSES, effectiveLimits } from './species.js';
import { findSampler, rampPick, samplerRamp } from './sampler.js';
import { makeRng } from './rng.js';
import { clamp, hash2, hexToPacked, mixPacked, packRGBA } from './util.js';

const SUPPORT_LAYERS = [SIZE_CLASSES.shrub.layer, SIZE_CLASSES.tree.layer];

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
  constructor(state) {
    this.state = state;
    this.world = new World(state.world);
    this.env = makeEnv(state);
    this.reset(state.seed);
  }

  reset(seed = this.state.seed) {
    this.world.configure(this.state.world);
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
      if (mine.length >= limits.maxInstances) continue;

      let attempts = sp.spawn.rate * dt;
      while (attempts > 0) {
        if (attempts >= 1 || this.rng.chance(attempts)) {
          this.trySpawn(sp, this.rng.int(0, this.world.cols - 1));
        }
        attempts -= 1;
      }

      for (const parent of mine) {
        if (!this.rng.chance(sp.spread.rate * dt)) continue;
        const dist = Math.round(this.rng.range(sp.spread.radiusMin, sp.spread.radiusMax));
        const col = parent.col + dist * this.rng.sign();
        this.trySpawn(sp, col);
      }
    }
  }

  trySpawn(sp, col) {
    const c = clamp(col | 0, 0, this.world.cols - 1);
    const limits = effectiveLimits(sp, this.state.classLimits);
    const layer = SIZE_CLASSES[sp.sizeClass].layer;
    if (this.world.hasNeighborWithin(layer, c, limits.minSpacing)) return null;

    const plant = new Plant({
      id: this.nextId,
      species: sp,
      limits,
      col: c,
      world: this.world,
      rng: makeRng(this.rng.seed()),
    });
    plant.layer = layer;
    const cells = this.world.footprint(c, 0, this.world.cellPx);
    if (!this.world.canClaim(layer, cells, plant.id)) return null;
    this.world.claim(layer, cells, plant.id);
    plant.cells = cells.slice();
    plant.grantedRadiusCells = 0;
    plant.grantedHeightPx = this.world.cellPx;
    this.nextId++;
    this.plants.push(plant);
    this.rasterQueue.push(plant);
    return plant;
  }

  requestSpace(plant, radiusCells, heightPx) {
    const cells = this.world.footprint(plant.col, radiusCells, heightPx);
    if (!this.world.canClaim(plant.layer, cells, plant.id)) return false;
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

  composite() {
    const w = this.world;
    const buf = this.buffer;
    this.paintBackground(buf);
    const order = [...this.plants].sort(
      (a, b) => SIZE_CLASSES[a.species.sizeClass].order - SIZE_CLASSES[b.species.sizeClass].order,
    );
    for (const plant of order) {
      const b = plant.bounds;
      if (b.x1 < b.x0) continue;
      // Mats sit on the soil line; anything with a stem is rooted just below it.
      const anchorY =
        plant.species.sizeClass === 'ground' ? w.surfacePx + 1 : w.surfacePx + Math.floor(w.cellPx / 2);
      const anchorX = plant.col * w.cellPx + Math.floor(w.cellPx / 2);
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
    this.bufferDirty = false;
    return buf;
  }

  paintBackground(buf) {
    const w = this.world;
    const cfg = this.state.world;
    const skyTop = hexToPacked(cfg.skyTop);
    const skyBottom = hexToPacked(cfg.skyBottom);
    for (let y = 0; y < w.surfacePx; y++) {
      const t = w.surfacePx > 1 ? y / (w.surfacePx - 1) : 0;
      const c = mixPacked(skyTop, skyBottom, t);
      buf.fill(c, y * w.pxW, (y + 1) * w.pxW);
    }
    // Soil is dithered out of its ramp rather than tiled, so the sampler art
    // does not show up as visible stripes across the ground.
    const sampler = findSampler(this.state.materials, cfg.soilSampler);
    const ramp = sampler ? samplerRamp(this.state.materials, sampler) : [];
    const fallback = packRGBA(52, 38, 28, 255);
    const soilDepth = Math.max(1, w.pxH - w.surfacePx);
    for (let y = w.surfacePx; y < w.pxH; y++) {
      const depth = (y - w.surfacePx) / soilDepth;
      for (let x = 0; x < w.pxW; x++) {
        let c = fallback;
        if (ramp.length) {
          const noise = (hash2(x, y, 7331) - 0.5) * 0.22;
          const surfaceLift = y < w.surfacePx + 2 ? 0.22 : 0;
          const t = clamp(0.72 - depth * 0.7 + noise + surfaceLift, 0, 1);
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
    findSupport: () => null,
  };
  const plant = new Plant({
    id: 1,
    species,
    limits,
    col: 0,
    world: fakeWorld,
    rng: makeRng(seed),
  });
  plant.layer = SIZE_CLASSES[species.sizeClass].layer;
  // No requestSpace in this context, so the plant grows to its own limits.
  plant.previewCtx = { world: fakeWorld, supportLayers: [] };
  return plant;
}

export { MAT };
