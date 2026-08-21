// The build planner.
//
// Nobody decides where the next building goes by hand: the planner scores
// every unlocked building against what the store is short of, checks that the
// settlement can plausibly pay for it, and then scores every legal cell for a
// site. Weights for all of it live in the build config, so a settlement can be
// pushed toward housing, industry or civic work by changing numbers.

import { clamp } from '../util.js';
import { BUILDINGS, scaledCost } from './buildings.js';
import { stockTargets } from './economy.js';
import { stockBulk } from './resources.js';

export function canPlaceAt(sim, def, col, row) {
  const spacing = Math.max(0, sim.cfg.build.spacing | 0);
  for (let r = row - spacing; r < row + def.h + spacing; r++) {
    for (let c = col - spacing; c < col + def.w + spacing; c++) {
      if (!sim.inBounds(c, r)) return false;
      const inFootprint = c >= col && c < col + def.w && r >= row && r < row + def.h;
      if (inFootprint) {
        if (!sim.terrain.isBuildable(c, r)) return false;
        if (sim.terrain.depositAt(c, r)) return false;
        if (sim.buildGrid[sim.idx(c, r)] !== 0) return false;
      } else if (sim.buildGrid[sim.idx(c, r)] !== 0) {
        return false;
      }
    }
  }
  // Somewhere to walk up to.
  let open = 0;
  for (let r = row - 1; r <= row + def.h; r++) {
    for (let c = col - 1; c <= col + def.w; c++) {
      const inside = c >= col && c < col + def.w && r >= row && r < row + def.h;
      if (!inside && sim.walkable(c, r)) open++;
    }
  }
  return open >= 2;
}

export function siteScore(sim, def, col, row) {
  let score = 0;
  if (def.site) {
    const [kind, arg] = def.site.split(':');
    if (kind === 'deposit') {
      const dep = sim.terrain.findDeposit(arg, col, row, def.radius || 10);
      if (!dep) return -Infinity;
      const d = Math.hypot(dep.col - col, dep.row - row);
      score += 12 - d;
    } else if (kind === 'fertile') {
      let fert = 0;
      const rad = def.fields || 2;
      for (let r = row - rad; r <= row + rad; r++) {
        for (let c = col - rad; c <= col + rad; c++) fert += sim.terrain.fertility(c, r);
      }
      if (fert < 1) return -Infinity;
      score += fert * 2;
    }
  }
  if (def.category === 'gather' && def.job && def.job.type === 'harvest') {
    // Camps want standing growth of the classes they cut.
    let mass = 0;
    for (const p of sim.plantSim.plants) {
      if (!def.job.classes.includes(p.species.sizeClass)) continue;
      const d = Math.hypot(p.col - col, p.row - row);
      if (d < (def.radius || 12)) mass += sim.plantMass(p) / (1 + d * 0.15);
    }
    if (mass < 2) return -Infinity;
    score += clamp(mass * 0.25, 0, 14);
  }
  const center = sim.center || { col: col, row: row };
  const dist = Math.hypot(col - center.col, row - center.row);
  score -= dist * 0.35;
  // Homes and workshops like to be near a store; gathering wants to be out.
  const store = sim.nearestStore(col, row);
  if (store) {
    const sd = Math.hypot(store.col - col, store.row - row);
    score -= def.category === 'gather' ? Math.max(0, sd - 14) * 0.4 : sd * 0.3;
  }
  return score;
}

export function findSiteNear(sim, def, col, row, radius) {
  let best = null;
  let bestScore = -Infinity;
  for (let r = row - radius; r <= row + radius; r++) {
    for (let c = col - radius; c <= col + radius; c++) {
      if (!canPlaceAt(sim, def, c, r)) continue;
      const score = siteScore(sim, def, c, r);
      if (score > bestScore) {
        bestScore = score;
        best = { col: c, row: r, score };
      }
    }
  }
  return best && bestScore > -Infinity ? best : null;
}

export function findSite(sim, def) {
  const center = sim.center || { col: sim.world.cols >> 1, row: sim.world.rows >> 1 };
  return findSiteNear(sim, def, center.col, center.row, Math.max(4, sim.cfg.build.sprawl | 0));
}

export function plan(sim) {
  const cfg = sim.cfg.build;
  if (!cfg.autoBuild) return;
  if (sim.sites.length >= cfg.maxSites) return;
  const want = planNext(sim);
  if (!want) return;
  const site = findSite(sim, want);
  if (!site) return;
  sim.placeBuilding(want.id, site.col, site.row);
}

// Scores every unlocked building against what the settlement is short of and
// returns the best one it can plausibly pay for.
// A settlement only wants so many of one thing. Homes answer to the housing
// need instead, so they are not capped here.
export function typeCap(def, cfg, pop) {
  // Homes follow the housing need and storage follows how full the store is,
  // so neither is capped by head count.
  if (def.housing || def.storage) return 99;
  const per = (cfg.perType || {})[def.category];
  if (!per) return 99;
  return 1 + Math.floor(pop / per);
}

export function planNext(sim) {
  const cfg = sim.cfg.build;
  const pop = sim.people.length;
  const targets = stockTargets(sim.cfg.economy, pop);
  const weights = cfg.weights;
  let best = null;
  let bestScore = 0.25;

  for (const def of BUILDINGS) {
    if (!def.base && !sim.unlocked.has(def.id)) continue;
    if (sim.countAll(def.id) - sim.countBuilt(def.id) > 0) continue;
    if (sim.countAll(def.id) >= typeCap(def, cfg, pop)) continue;
    // No second workshop while the first one still has an empty bench.
    if ((def.slots || 0) > 0 && sim.countBuilt(def.id) > 0 && sim.workSlots(def.id) > 0) continue;
    let score = 0;

    if (def.housing) {
      const short = pop + cfg.housingSlack - sim.housingCapacity();
      if (short <= 0) continue;
      score = clamp(short / Math.max(1, def.housing), 0, 3) * 1.6;
      // Prefer the best home the settlement can actually supply.
      score *= 0.6 + def.comfort * 0.8;
    } else if (def.storage) {
      const cap = sim.storeCapacity();
      const fill = cap > 0 ? stockBulk(sim.stock) / cap : 1;
      if (fill < 0.7 && cap > 0) continue;
      score = 1.4 + (cap === 0 ? 2 : 0);
    } else if (def.job && def.job.type === 'harvest') {
      const res = Object.keys(def.job.yields)[0];
      const need = clamp((targets[res] - (sim.stock[res] || 0)) / Math.max(1, targets[res]), -1, 1);
      const open = sim.workSlots(def.id);
      score = need * 1.5 - open * 0.35;
      if (res === 'food') score += 0.4;
    } else if (def.job && def.job.type === 'mine') {
      const dep = sim.terrain.countDeposits(def.job.deposit);
      if (!dep.cells) continue;
      const res = Object.keys(def.job.yields)[0];
      const need = clamp((targets[res] - (sim.stock[res] || 0)) / Math.max(1, targets[res]), -1, 1);
      score = need * 1.5 - sim.workSlots(def.id) * 0.35;
    } else if (def.job && def.job.type === 'farm') {
      const need = clamp((targets.food - (sim.stock.food || 0)) / Math.max(1, targets.food), -1, 1);
      score = need * 1.7 - sim.workSlots(def.id) * 0.3;
    } else if (def.job && def.job.type === 'craft') {
      let inputs = 1;
      for (const [res, n] of Object.entries(def.job.in)) {
        inputs = Math.min(inputs, (sim.stock[res] || 0) / Math.max(1, n * 6));
      }
      let want = 0;
      for (const res of Object.keys(def.job.out)) {
        want += clamp((targets[res] - (sim.stock[res] || 0)) / Math.max(1, targets[res]), -1, 1);
      }
      score = want * inputs * 1.4 - sim.workSlots(def.id) * 0.3;
    } else if (def.job && def.job.type === 'research') {
      score = 1.1 - sim.countBuilt(def.id) * 0.6 - sim.workSlots(def.id) * 0.3;
    } else if (def.job && def.job.type === 'trade') {
      score = 1 - sim.countBuilt(def.id) * 1.5;
    } else if (def.health) {
      score = 0.9 - sim.countBuilt(def.id) * 0.5;
    }

    score *= weights[def.category] ?? 1;
    // Only plan what the settlement can supply: everything either in store
    // already or made by something that is standing.
    const cost = scaledCost(def, cfg);
    let feasible = true;
    for (const [res, n] of Object.entries(cost)) {
      const have = sim.stock[res] || 0;
      if (have >= n) continue;
      if (have >= n * 0.34 && producerOf(sim, res)) continue;
      if (producerOf(sim, res) && have >= n * 0.15) continue;
      feasible = false;
      break;
    }
    if (!feasible) continue;
    if (score > bestScore) {
      bestScore = score;
      best = def;
    }
  }
  return best;
}

export function producerOf(sim, res) {
  for (const b of sim.buildings) {
    if (!b.built || !b.def.job) continue;
    const job = b.def.job;
    if (job.yields && job.yields[res]) return b;
    if (job.out && job.out[res]) return b;
  }
  return null;
}
