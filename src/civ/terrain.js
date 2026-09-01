// Procedural terrain for the settlement map.
//
// Two noise fields (elevation and moisture) decide what every cell is, and
// resource deposits are scattered into the cells that suit them: stone and ore
// in the high rock, clay along the water. Deposits hold a finite amount, so a
// settlement that has emptied the ground near it has to reach further out.
//
// Everything here is a pure function of the seed and the parameters, so the
// same seed always rebuilds the same map.

import { clamp, clamp01, hash2, lerp, smoothstep } from '../util.js';
import { makeRng } from '../rng.js';

export const CELL = { grass: 0, water: 1, rock: 2, sand: 3 };

export const DEPOSIT_KINDS = ['stone', 'clay', 'ore'];

export function defaultTerrainConfig() {
  return {
    scale: 14,
    octaves: 4,
    persistence: 0.5,
    warp: 0.35,
    waterLevel: 0.32,
    sandBand: 0.04,
    rockLevel: 0.68,
    moistScale: 22,
    fertility: 0.6,
    // Wild growth simulated before the people arrive, in simulated seconds.
    warmup: 420,
    // How lush the map is: scales seeding rate and how many plants of each
    // species the land carries. Wild food and timber both follow this.
    wildness: 2.2,
    deposits: {
      stone: { density: 0.9, clusterMin: 2, clusterMax: 6, amountMin: 90, amountMax: 260 },
      clay: { density: 0.7, clusterMin: 2, clusterMax: 5, amountMin: 70, amountMax: 200 },
      ore: { density: 0.35, clusterMin: 1, clusterMax: 4, amountMin: 60, amountMax: 180 },
    },
  };
}

// Value noise with bilinear interpolation, summed over octaves. hash2 is the
// same stable hash the plant shading uses, so terrain is reproducible without
// carrying an RNG stream through the sampling.
function valueNoise(x, y, seed) {
  const x0 = Math.floor(x);
  const y0 = Math.floor(y);
  const fx = smoothstep(x - x0);
  const fy = smoothstep(y - y0);
  const a = hash2(x0, y0, seed);
  const b = hash2(x0 + 1, y0, seed);
  const c = hash2(x0, y0 + 1, seed);
  const d = hash2(x0 + 1, y0 + 1, seed);
  return lerp(lerp(a, b, fx), lerp(c, d, fx), fy);
}

export function fbm(x, y, seed, octaves, persistence) {
  let sum = 0;
  let amp = 1;
  let norm = 0;
  let freq = 1;
  for (let o = 0; o < octaves; o++) {
    sum += valueNoise(x * freq, y * freq, seed + o * 7919) * amp;
    norm += amp;
    amp *= persistence;
    freq *= 2;
  }
  return norm > 0 ? sum / norm : 0;
}

export class Terrain {
  constructor(world, cfg, seed) {
    this.world = world;
    this.cfg = cfg;
    this.seed = seed >>> 0;
    this.generate();
  }

  generate() {
    const { cols, rows } = this.world;
    const cfg = this.cfg;
    const n = cols * rows;
    this.cols = cols;
    this.rows = rows;
    this.elev = new Float32Array(n);
    this.moist = new Float32Array(n);
    this.fert = new Float32Array(n);
    this.type = new Uint8Array(n);
    this.depositIndex = new Int32Array(n);
    this.deposits = [];

    const scale = Math.max(2, cfg.scale);
    const mscale = Math.max(2, cfg.moistScale);
    const oct = clamp(cfg.octaves | 0, 1, 6);
    const pers = clamp(cfg.persistence, 0.1, 0.9);

    for (let r = 0; r < rows; r++) {
      for (let c = 0; c < cols; c++) {
        const i = r * cols + c;
        // Domain warp so coastlines meander instead of following the noise
        // grid; without it lakes come out suspiciously round.
        const wx = fbm(c / (scale * 2), r / (scale * 2), this.seed + 101, 2, 0.5) - 0.5;
        const wy = fbm(c / (scale * 2), r / (scale * 2), this.seed + 202, 2, 0.5) - 0.5;
        const x = c / scale + wx * cfg.warp * 2;
        const y = r / scale + wy * cfg.warp * 2;
        const e = fbm(x, y, this.seed, oct, pers);
        const m = fbm(c / mscale + 31.7, r / mscale + 12.3, this.seed + 5501, 3, 0.55);
        this.elev[i] = e;
        this.moist[i] = m;
        if (e < cfg.waterLevel) this.type[i] = CELL.water;
        else if (e < cfg.waterLevel + cfg.sandBand) this.type[i] = CELL.sand;
        else if (e > cfg.rockLevel) this.type[i] = CELL.rock;
        else this.type[i] = CELL.grass;
        const wetness = clamp01(m * 0.7 + (1 - Math.abs(e - cfg.waterLevel - 0.12) * 2.2) * 0.5);
        this.fert[i] = this.type[i] === CELL.grass ? clamp01(wetness * cfg.fertility * 1.6) : 0;
      }
    }

    this.scatterDeposits();
    this.waterCells = 0;
    for (let i = 0; i < n; i++) if (this.type[i] === CELL.water) this.waterCells++;
  }

  scatterDeposits() {
    const rng = makeRng(this.seed ^ 0x5bf03635);
    const { cols, rows } = this;
    const area = cols * rows;
    for (const kind of DEPOSIT_KINDS) {
      const dc = this.cfg.deposits[kind];
      if (!dc) continue;
      const clusters = Math.round((dc.density * area) / 100);
      for (let k = 0; k < clusters; k++) {
        const seedCell = this.pickSeedCell(kind, rng);
        if (seedCell < 0) continue;
        const size = rng.int(dc.clusterMin, dc.clusterMax);
        this.growCluster(seedCell, size, kind, dc, rng);
      }
    }
  }

  // Deposits sit where their story puts them: stone and ore in high ground,
  // clay in the damp low ground next to water.
  pickSeedCell(kind, rng) {
    for (let tries = 0; tries < 60; tries++) {
      const c = rng.int(0, this.cols - 1);
      const r = rng.int(0, this.rows - 1);
      const i = r * this.cols + c;
      if (this.type[i] === CELL.water) continue;
      if (this.depositIndex[i] !== 0) continue;
      const e = this.elev[i];
      if (kind === 'stone' && (this.type[i] === CELL.rock || e > this.cfg.rockLevel - 0.12)) return i;
      if (kind === 'ore' && this.type[i] === CELL.rock && rng.chance(0.7)) return i;
      if (kind === 'clay' && this.nearWater(c, r, 3) && this.type[i] !== CELL.rock) return i;
    }
    return -1;
  }

  growCluster(seedCell, size, kind, dc, rng) {
    let c = seedCell % this.cols;
    let r = (seedCell / this.cols) | 0;
    for (let n = 0; n < size; n++) {
      const i = r * this.cols + c;
      if (
        c >= 0 && c < this.cols && r >= 0 && r < this.rows &&
        this.type[i] !== CELL.water && this.depositIndex[i] === 0
      ) {
        const amount = rng.int(dc.amountMin, dc.amountMax);
        this.deposits.push({
          id: this.deposits.length + 1,
          kind,
          col: c,
          row: r,
          amount,
          max: amount,
          seed: rng.seed(),
        });
        this.depositIndex[i] = this.deposits.length;
      }
      c += rng.int(-1, 1);
      r += rng.int(-1, 1);
      c = clamp(c, 0, this.cols - 1);
      r = clamp(r, 0, this.rows - 1);
    }
  }

  idx(c, r) {
    return r * this.cols + c;
  }

  inBounds(c, r) {
    return c >= 0 && c < this.cols && r >= 0 && r < this.rows;
  }

  typeAt(c, r) {
    return this.inBounds(c, r) ? this.type[this.idx(c, r)] : CELL.water;
  }

  isWater(c, r) {
    return this.typeAt(c, r) === CELL.water;
  }

  isBuildable(c, r) {
    const t = this.typeAt(c, r);
    return t === CELL.grass || t === CELL.sand || t === CELL.rock;
  }

  fertility(c, r) {
    return this.inBounds(c, r) ? this.fert[this.idx(c, r)] : 0;
  }

  nearWater(c, r, radius) {
    for (let y = r - radius; y <= r + radius; y++) {
      for (let x = c - radius; x <= c + radius; x++) {
        if (this.inBounds(x, y) && this.type[this.idx(x, y)] === CELL.water) return true;
      }
    }
    return false;
  }

  depositAt(c, r) {
    if (!this.inBounds(c, r)) return null;
    const di = this.depositIndex[this.idx(c, r)];
    return di > 0 ? this.deposits[di - 1] : null;
  }

  // Nearest deposit of a kind with anything left in it.
  findDeposit(kind, col, row, radius) {
    let best = null;
    let bestD = Infinity;
    for (const d of this.deposits) {
      if (d.kind !== kind || d.amount <= 0) continue;
      const dx = d.col - col;
      const dy = d.row - row;
      const dist = dx * dx + dy * dy;
      if (dist > radius * radius || dist >= bestD) continue;
      best = d;
      bestD = dist;
    }
    return best;
  }

  countDeposits(kind) {
    let cells = 0;
    let amount = 0;
    for (const d of this.deposits) {
      if (d.kind !== kind) continue;
      if (d.amount > 0) cells++;
      amount += d.amount;
    }
    return { cells, amount };
  }

  take(deposit, n) {
    const got = Math.min(deposit.amount, n);
    deposit.amount -= got;
    if (deposit.amount <= 0) this.depositIndex[this.idx(deposit.col, deposit.row)] = 0;
    return got;
  }

  // A tolerable spot for the first storehouse: flat, buildable, near fertile
  // ground and not on top of a deposit.
  findStartCell(rng) {
    let best = null;
    let bestScore = -Infinity;
    for (let tries = 0; tries < 400; tries++) {
      const c = rng.int(2, this.cols - 3);
      const r = rng.int(2, this.rows - 3);
      if (!this.isBuildable(c, r) || this.depositAt(c, r)) continue;
      let score = this.fertility(c, r) * 2;
      let openness = 0;
      for (let y = r - 2; y <= r + 2; y++) {
        for (let x = c - 2; x <= c + 2; x++) {
          if (this.isBuildable(x, y) && !this.depositAt(x, y)) openness++;
        }
      }
      score += openness / 25;
      if (this.nearWater(c, r, 6)) score += 0.5;
      // Middle of the map reads better than a corner.
      const dx = (c - this.cols / 2) / this.cols;
      const dy = (r - this.rows / 2) / this.rows;
      score -= Math.sqrt(dx * dx + dy * dy);
      if (score > bestScore) {
        bestScore = score;
        best = { col: c, row: r };
      }
    }
    return best || { col: this.cols >> 1, row: this.rows >> 1 };
  }
}
