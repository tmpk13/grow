// What a settler does with the next second of their life.
//
// Every task is a small state machine with a phase and a target: walk there,
// work, carry the result somewhere. Nothing here reads a global; the
// settlement is passed in, which keeps the decision making testable and keeps
// the settlement itself down to world, buildings and books.
//
// The one rule that shapes all of it: material only moves because a person
// carried it. A wall goes up because somebody walked wood to the site.

import { clamp, clamp01 } from '../util.js';
import { carryLimit, isWorkTime } from './people.js';
import { buyFood, payWage, recordConsumed, recordProduced } from './economy.js';
import { takeStock } from './resources.js';

export function updatePerson(sim, p, dt) {
  const cfg = sim.cfg;
  const pcfg = cfg.people;
  p.age += (dt / pcfg.dayLength) * pcfg.yearsPerDay;
  p.adultAge = pcfg.adultAge;

  const working = !!p.task && p.task.kind !== 'sleep' && p.task.kind !== 'idle';
  p.tickNeeds(dt, pcfg, working);

  if (p.age > p.lifespan) {
    p.alive = false;
    p.cause = 'old age';
    return;
  }
  if (p.health <= 0) {
    p.alive = false;
    p.cause = 'hunger';
    return;
  }
  const sick = pcfg.sicknessRate * dt / Math.max(1, pcfg.dayLength) * (1 - sim.wellCoverage(p) * 0.6);
  if (sick > 0 && sim.rng.chance(sick * (1.6 - p.health))) {
    p.alive = false;
    p.cause = 'sickness';
    return;
  }

  // Children eat from the same store the adults fill; they simply do not work
  // for it. Without this they starve in their first days.
  p.eatCooldown = Math.max(0, (p.eatCooldown || 0) - dt);
  if (!p.adult) {
    if (p.hunger > pcfg.eatAt && !p.eatCooldown && (!p.task || p.task.kind !== 'eat')) {
      abandonTask(sim, p);
      if (!startEat(sim, p)) p.eatCooldown = 4;
    }
    if (p.task && p.task.kind === 'eat') {
      runTask(sim, p, dt);
      return;
    }
    childBehavior(sim, p, dt);
    return;
  }

  const night = !isWorkTime(sim.time, pcfg);
  if (p.sleeping) {
    if (!night || p.energy >= 1) {
      p.sleeping = false;
      p.clearTask();
    } else {
      return;
    }
  }
  if (p.hunger > pcfg.eatAt && !p.eatCooldown && (!p.task || p.task.kind !== 'eat')) {
    abandonTask(sim, p);
    // With nothing in the store, going hungry is not a reason to stand still:
    // the person falls through to work, which for a hungry settlement means
    // somebody goes out looking for food. The cooldown keeps a failed attempt
    // from cancelling that work on the very next tick.
    if (!startEat(sim, p)) p.eatCooldown = 4;
  } else if (night && (!p.task || (p.task.kind !== 'sleep' && p.task.kind !== 'eat'))) {
    abandonTask(sim, p);
    startSleep(sim, p);
  }
  if (!p.task) chooseTask(sim, p);
  if (p.task) runTask(sim, p, dt);
}

export function childBehavior(sim, p, dt) {
  if (!p.task || p.task.kind !== 'idle') {
    const home = sim.buildings.find((b) => b.id === p.home);
    const anchor = home ? sim.accessCell(home) : { col: p.cellCol, row: p.cellRow };
    const c = clamp(anchor.col + sim.rng.int(-3, 3), 0, sim.world.cols - 1);
    const r = clamp(anchor.row + sim.rng.int(-3, 3), 0, sim.world.rows - 1);
    p.task = { kind: 'idle', timer: sim.rng.range(1, 4) };
    if (sim.walkable(c, r)) p.setPath(sim.findPath(p.cellCol, p.cellRow, c, r));
  }
  const arrived = walk(sim, p, dt, 0.7);
  if (arrived) {
    p.task.timer -= dt;
    if (p.task.timer <= 0) p.clearTask();
  }
}

export function walk(sim, p, dt, speedScale = 1) {
  const pcfg = sim.cfg.people;
  const road = sim.traffic[sim.idx(clamp(p.cellCol, 0, sim.world.cols - 1), clamp(p.cellRow, 0, sim.world.rows - 1))] || 0;
  const speed = pcfg.walkSpeed * speedScale *
    (1 + clamp01(road / 6) * pcfg.roadSpeedBonus) *
    (0.7 + p.energy * 0.3) *
    (0.6 + p.health * 0.4);
  const before = p.path ? 1 : 0;
  const done = p.moveAlong(dt, speed);
  if (before) {
    const i = sim.idx(clamp(p.cellCol, 0, sim.world.cols - 1), clamp(p.cellRow, 0, sim.world.rows - 1));
    sim.traffic[i] = Math.min(20, sim.traffic[i] + dt * 2);
  }
  return done;
}

// Walks to a building, trying its other free sides when the first one cannot
// be reached. A single unreachable spot used to be enough to starve somebody
// standing next to a full store.
export function pathToBuilding(sim, p, b) {
  const cells = sim.accessCells(b);
  for (let i = 0; i < Math.min(4, cells.length); i++) {
    if (pathTo(sim, p, cells[i].col, cells[i].row)) return true;
  }
  return false;
}

export function pathTo(sim, p, col, row) {
  const path = sim.findPath(p.cellCol, p.cellRow, col, row);
  if (!path) return false;
  p.setPath(path);
  return true;
}

export function abandonTask(sim, p) {
  const t = p.task;
  if (!t) return;
  if (t.kind === 'haul') {
    if (t.phase === 'toSource') sim.releaseStock(t.res, t.amount);
    const dest = sim.buildings.find((b) => b.id === t.toId);
    if (dest) {
      if (t.target === 'site') dest.incoming[t.res] = Math.max(0, (dest.incoming[t.res] || 0) - t.amount);
      else if (t.target === 'input') dest.reservedIn[t.res] = Math.max(0, (dest.reservedIn[t.res] || 0) - t.amount);
      else if (t.target === 'output') dest.reservedOut[t.res] = Math.max(0, (dest.reservedOut[t.res] || 0) - t.amount);
    }
  }
  if (t.kind === 'harvest') {
    const plant = sim.plantSim.plants.find((x) => x.id === t.plantId);
    if (plant && plant.claimedBy === p.id) plant.claimedBy = 0;
  }
  if (t.kind === 'pickup') {
    const pile = sim.piles.find((q) => q.id === t.pileId);
    if (pile && pile.claimedBy === p.id) pile.claimedBy = 0;
  }
  if (t.kind === 'build') {
    const site = sim.buildings.find((b) => b.id === t.buildingId);
    if (site) site.builders = Math.max(0, (site.builders || 0) - 1);
  }
  p.clearTask();
}

// Tries every store, nearest first: an unreachable one is not a reason to go
// hungry while another has food.
export function startEat(sim, p) {
  if ((sim.stock.food || 0) < 1) return false;
  const stores = sim.buildings
    .filter((b) => b.built && b.def.isStore)
    .sort((a, z) => (a.col - p.x) ** 2 + (a.row - p.y) ** 2 - ((z.col - p.x) ** 2 + (z.row - p.y) ** 2));
  for (const store of stores) {
    if (!pathToBuilding(sim, p, store)) continue;
    p.task = { kind: 'eat', toId: store.id, phase: 'toStore' };
    return true;
  }
  return false;
}

export function startSleep(sim, p) {
  const home = sim.buildings.find((b) => b.id === p.home && b.built);
  if (!home) {
    p.sleeping = true;
    p.task = { kind: 'sleep', phase: 'sleeping' };
    return;
  }
  p.task = { kind: 'sleep', buildingId: home.id, phase: 'toHome' };
  if (!pathToBuilding(sim, p, home)) {
    p.sleeping = true;
    p.task.phase = 'sleeping';
  }
}

export function chooseTask(sim, p) {
  if (p.carrying) {
    startDeliver(sim, p);
    return;
  }
  // A settlement that is running out of food puts everyone on food, whatever
  // their trade, until the store is off the floor again.
  const pop = Math.max(1, sim.people.length);
  const foodShort = (sim.stock.food || 0) < pop * sim.cfg.people.mealSize * 2;
  const work = sim.buildings.find((b) => b.id === p.work && b.built);
  const foodWork = !!work && !!work.def.job && producesFood(work.def.job);
  if (foodShort && !foodWork && startForage(sim, p)) return;
  if (work && work.def.job) {
    const job = work.def.job;
    if (job.type === 'harvest' && startHarvest(sim, p, work)) return;
    if (job.type === 'mine' && startMine(sim, p, work)) return;
    if (job.type === 'farm' || job.type === 'research' || job.type === 'trade') {
      startStation(sim, p, work);
      return;
    }
    if (job.type === 'craft') {
      if (sim.craftReady(work)) {
        startStation(sim, p, work);
        return;
      }
    }
  }
  if (startLabor(sim, p)) return;
  // Nothing queued: forage if the store is thin on food, otherwise wander.
  if ((sim.stock.food || 0) < sim.people.length * sim.cfg.people.mealSize) {
    if (startForage(sim, p)) return;
  }
  startWander(sim, p);
}

// Foraging without a hut: anyone can strip the low growth for something to
// eat, which is what keeps a settlement alive before it has built anything.
export function startForage(sim, p) {
  const wild = {
    radius: 30,
    classes: ['ground', 'herb', 'vine'],
    yields: { food: 1, fiber: 0.3 },
    regrow: 0.35,
  };
  return startHarvest(sim, p, null, wild);
}

function outLoad(b) {
  let total = 0;
  for (const n of Object.values(b.out)) total += n;
  return total;
}

function producesFood(job) {
  const out = job.out || job.yields || {};
  return !!out.food;
}

export function startWander(sim, p) {
  const c = clamp(p.cellCol + sim.rng.int(-4, 4), 0, sim.world.cols - 1);
  const r = clamp(p.cellRow + sim.rng.int(-4, 4), 0, sim.world.rows - 1);
  p.task = { kind: 'idle', timer: sim.rng.range(1.5, 5) };
  if (sim.walkable(c, r)) pathTo(sim, p, c, r);
}

// Picks the plant with the best mass for the walk, so camps eat their way
// outward instead of everyone crossing the map for one tree.
export function startHarvest(sim, p, work, jobOverride) {
  const job = jobOverride || work.def.job;
  const near = (jobOverride ? jobOverride.radius : work.def.radius) || 12;
  const origin = work ? { col: work.col, row: work.row } : { col: p.cellCol, row: p.cellRow };
  // Two passes: the camp's own range first, then a long walk when the ground
  // around it has been stripped bare.
  const best = pickPlant(sim, p, job, origin, near) || pickPlant(sim, p, job, origin, near * 3);
  if (!best) return false;
  const spot = sim.freeCellNear(best.col, best.row);
  if (!spot || !pathTo(sim, p, spot.col, spot.row)) return false;
  best.claimedBy = p.id;
  p.task = {
    kind: 'harvest',
    plantId: best.id,
    yields: job.yields,
    regrow: job.regrow || 0,
    phase: 'toPlant',
    timer: 0,
  };
  return true;
}

// Best mass for the walk, so a camp works outward from itself.
function pickPlant(sim, p, job, origin, radius) {
  let best = null;
  let bestScore = 0;
  for (const plant of sim.plantSim.plants) {
    if (!job.classes.includes(plant.species.sizeClass)) continue;
    if (plant.claimedBy && plant.claimedBy !== p.id) continue;
    const d = Math.hypot(plant.col - origin.col, plant.row - origin.row);
    if (d > radius) continue;
    const mass = sim.plantMass(plant);
    if (mass < sim.cfg.work.minHarvestMass) continue;
    const score = mass / (2 + d);
    if (score > bestScore) {
      bestScore = score;
      best = plant;
    }
  }
  return best;
}

export function startMine(sim, p, work) {
  const job = work.def.job;
  const dep = sim.terrain.findDeposit(job.deposit, work.col, work.row, work.def.radius || 12);
  if (!dep) return false;
  const spot = sim.freeCellNear(dep.col, dep.row);
  if (!spot || !pathTo(sim, p, spot.col, spot.row)) return false;
  p.task = { kind: 'mine', depositId: dep.id, yields: job.yields, phase: 'toNode', timer: 0 };
  return true;
}

export function startStation(sim, p, work) {
  const at = sim.workSpot(work, p);
  p.task = { kind: 'station', buildingId: work.id, phase: 'toWork' };
  if (pathTo(sim, p, at.col, at.row)) return true;
  if (pathToBuilding(sim, p, work)) return true;
  p.clearTask();
  return false;
}

export function startDeliver(sim, p) {
  const store = sim.nearestStore(p.cellCol, p.cellRow);
  if (!store) {
    p.task = { kind: 'idle', timer: 2 };
    return;
  }
  p.task = { kind: 'deliver', toId: store.id, phase: 'toStore' };
  if (!pathToBuilding(sim, p, store)) {
    sim.deposit(p.carry.res, p.carry.n);
    p.drop();
    p.clearTask();
  }
}

// Laborers keep the sites and workshops fed. The scan is cheap enough to run
// per idle person, and it always picks the nearest useful thing to do.
export function startLabor(sim, p) {
  const cap = carryLimit(sim.cfg.people, sim.mods);
  // Every option is collected and tried in order rather than only the best
  // one: a single unreachable load used to block hauling and building
  // entirely, because the person kept picking it and kept failing to path.
  const options = [];
  const consider = (score, make) => {
    make.score = score;
    options.push(make);
  };

  for (const pile of sim.piles) {
    if (pile.claimedBy && pile.claimedBy !== p.id) continue;
    if (!sim.wanted(pile.res)) continue;
    const d = Math.hypot(pile.col - p.x, pile.row - p.y);
    consider(19 - d * 0.2, { kind: 'pickup', pileId: pile.id });
  }

  for (const site of sim.buildings) {
    if (site.built) continue;
    const d = Math.hypot(site.col - p.x, site.row - p.y);
    for (const [res, need] of Object.entries(site.cost)) {
      const have = (site.delivered[res] || 0) + (site.incoming[res] || 0);
      const short = need - have;
      if (short <= 0) continue;
      const take = Math.min(short, cap, sim.availableStock(res));
      if (take <= 0) continue;
      consider(20 - d * 0.2, { kind: 'haul', res, amount: take, toId: site.id, target: 'site' });
    }
    if (sim.siteReady(site) && (site.builders || 0) < 3) {
      consider(24 - d * 0.2, { kind: 'build', buildingId: site.id });
    }
  }

  for (const b of sim.buildings) {
    if (!b.built || !b.def.job) continue;
    const d = Math.hypot(b.col - p.x, b.row - p.y);
    if (b.def.job.type === 'craft' && b.workers.length) {
      for (const [res, n] of Object.entries(b.def.job.in)) {
        const want = Math.ceil(n * 3 * sim.cfg.work.restockShare);
        const have = (b.inv[res] || 0) + (b.reservedIn[res] || 0);
        if (have >= want) continue;
        const take = Math.min(want - have, cap, sim.availableStock(res));
        if (take <= 0) continue;
        consider(14 - d * 0.2, { kind: 'haul', res, amount: take, toId: b.id, target: 'input' });
      }
    }
    for (const [res, n] of Object.entries(b.out)) {
      const free = n - (b.reservedOut[res] || 0);
      if (free < 1) continue;
      consider(16 - d * 0.2, {
        kind: 'haul',
        res,
        amount: Math.min(free, cap),
        fromId: b.id,
        target: 'output',
      });
    }
  }

  options.sort((a, z) => z.score - a.score);
  for (const option of options.slice(0, 8)) {
    if (takeLaborTask(sim, p, option)) return true;
  }
  return false;
}

function takeLaborTask(sim, p, best) {
  if (best.kind === 'pickup') {
    const pile = sim.piles.find((q) => q.id === best.pileId);
    if (!pile) return false;
    const spot = sim.freeCellNear(pile.col, pile.row);
    if (!spot || !pathTo(sim, p, spot.col, spot.row)) return false;
    pile.claimedBy = p.id;
    p.task = { kind: 'pickup', pileId: pile.id, phase: 'toPile' };
    return true;
  }
  if (best.kind === 'build') {
    const site = sim.buildings.find((b) => b.id === best.buildingId);
    if (!pathToBuilding(sim, p, site)) return false;
    site.builders = (site.builders || 0) + 1;
    p.task = { kind: 'build', buildingId: site.id, phase: 'toSite' };
    return true;
  }

  if (best.target === 'output') {
    const from = sim.buildings.find((b) => b.id === best.fromId);
    if (!pathToBuilding(sim, p, from)) return false;
    from.reservedOut[best.res] = (from.reservedOut[best.res] || 0) + best.amount;
    p.task = {
      kind: 'haul',
      res: best.res,
      amount: best.amount,
      fromId: from.id,
      target: 'output',
      phase: 'toSource',
    };
    return true;
  }

  const dest = sim.buildings.find((b) => b.id === best.toId);
  const store = sim.nearestStore(p.cellCol, p.cellRow);
  if (!store || !dest) return false;
  if (!pathToBuilding(sim, p, store)) return false;
  sim.reserveStock(best.res, best.amount);
  if (best.target === 'site') dest.incoming[best.res] = (dest.incoming[best.res] || 0) + best.amount;
  else dest.reservedIn[best.res] = (dest.reservedIn[best.res] || 0) + best.amount;
  p.task = {
    kind: 'haul',
    res: best.res,
    amount: best.amount,
    fromId: store.id,
    toId: dest.id,
    target: best.target,
    phase: 'toSource',
  };
  return true;
}

export function runTask(sim, p, dt) {
  const t = p.task;
  const cfg = sim.cfg;
  switch (t.kind) {
    case 'idle': {
      if (walk(sim, p, dt, 0.8)) {
        t.timer -= dt;
        if (t.timer <= 0) p.clearTask();
      }
      break;
    }
    case 'sleep': {
      if (t.phase === 'toHome') {
        if (walk(sim, p, dt)) {
          t.phase = 'sleeping';
          p.sleeping = true;
        }
      }
      break;
    }
    case 'eat': {
      if (!walk(sim, p, dt)) break;
      const meal = cfg.people.mealSize * (p.adult ? 1 : 0.6);
      const got = buyFood(sim.econ, cfg.economy, p, sim.stock, meal, sim.hasMarket());
      if (got > 0) p.eat(got);
      else p.happiness = clamp01(p.happiness - 0.1);
      p.clearTask();
      break;
    }
    case 'harvest': {
      const plant = sim.plantSim.plants.find((x) => x.id === t.plantId);
      if (!plant || !plant.alive) {
        p.clearTask();
        break;
      }
      if (t.phase === 'toPlant') {
        if (walk(sim, p, dt)) t.phase = 'cutting';
        break;
      }
      const rate = cfg.work.harvestRate * (sim.mods.gather || 1) * cfg.people.workRate * p.skill;
      t.timer += rate * dt;
      doWork(sim, p, rate * dt);
      const mass = sim.plantMass(plant);
      if (t.timer >= mass) {
        const cap = carryLimit(cfg.people, sim.mods);
        // A mat that is cut back only gives up the part that was taken; a
        // felled tree gives up all of it.
        const cutBack = t.regrow > 0 && plant.species.sizeClass === 'ground';
        const gain = mass * (cutBack ? 1 - t.regrow : 1) * (sim.mods.yield || 1);
        // Whatever will not fit on one person stays where it fell and has to
        // be carried in later: felling a tree is not the same as having the
        // timber in the store.
        for (const [res, per] of Object.entries(t.yields)) {
          // Nothing is stripped from a plant that the settlement has no use
          // for; a byproduct it is drowning in is simply left on the plant.
          if (!sim.wanted(res)) continue;
          const total = Math.max(1, Math.round(gain * per));
          const room = p.carry.res && p.carry.res !== res ? 0 : cap - p.carry.n;
          const take = Math.max(0, Math.min(total, room));
          if (take > 0) p.pick(res, take);
          if (total - take > 0) sim.addPile(plant.col, plant.row, res, total - take);
          recordProduced(sim.econ, res, total);
        }
        if (cutBack) {
          plant.radiusPx *= t.regrow;
          plant.heightPx *= t.regrow;
          plant.confinedSide = false;
          plant.dirty = true;
          plant.claimedBy = 0;
          sim.plantSim.rasterQueue.push(plant);
        } else {
          const at = sim.plantSim.plants.indexOf(plant);
          if (at !== -1) sim.plantSim.removePlantAt(at);
        }
        p.clearTask();
        if (p.carry.n >= carryLimit(cfg.people, sim.mods) * 0.9) startDeliver(sim, p);
      }
      break;
    }
    case 'mine': {
      const dep = sim.terrain.deposits.find((d) => d.id === t.depositId);
      if (!dep || dep.amount <= 0) {
        p.clearTask();
        break;
      }
      if (t.phase === 'toNode') {
        if (walk(sim, p, dt)) t.phase = 'digging';
        break;
      }
      const rate = cfg.work.mineRate * (sim.mods.gather || 1) * cfg.people.workRate * p.skill;
      doWork(sim, p, rate * dt);
      t.timer += rate * dt;
      const cap = carryLimit(cfg.people, sim.mods);
      while (t.timer >= 1 && p.carry.n < cap && dep.amount > 0) {
        t.timer -= 1;
        const got = sim.terrain.take(dep, 1);
        if (got > 0) {
          for (const [res, per] of Object.entries(t.yields)) {
            const n = Math.max(1, Math.round(per * (sim.mods.yield || 1)));
            p.pick(res, n);
            recordProduced(sim.econ, res, n);
          }
        }
      }
      if (p.carry.n >= cap || dep.amount <= 0) {
        p.clearTask();
        if (p.carrying) startDeliver(sim, p);
      }
      break;
    }
    case 'pickup': {
      const pile = sim.piles.find((q) => q.id === t.pileId);
      if (!pile) {
        p.clearTask();
        break;
      }
      if (!walk(sim, p, dt)) break;
      const cap = carryLimit(cfg.people, sim.mods);
      const room = p.carry.res && p.carry.res !== pile.res ? 0 : cap - p.carry.n;
      const got = sim.takePile(pile, room);
      if (got > 0) p.pick(pile.res, got);
      pile.claimedBy = 0;
      p.clearTask();
      if (p.carrying) startDeliver(sim, p);
      break;
    }
    case 'deliver': {
      if (!walk(sim, p, dt)) break;
      const load = p.drop();
      if (load.res) {
        const put = sim.deposit(load.res, load.n);
        if (put < load.n) p.pick(load.res, load.n - put);
      }
      p.clearTask();
      break;
    }
    case 'haul': {
      if (t.phase === 'toSource') {
        if (!walk(sim, p, dt)) break;
        if (t.target === 'output') {
          const from = sim.buildings.find((b) => b.id === t.fromId);
          if (!from) {
            abandonTask(sim, p);
            break;
          }
          const got = Math.min(t.amount, from.out[t.res] || 0);
          from.out[t.res] = (from.out[t.res] || 0) - got;
          from.reservedOut[t.res] = Math.max(0, (from.reservedOut[t.res] || 0) - t.amount);
          if (got <= 0) {
            p.clearTask();
            break;
          }
          p.pick(t.res, got);
          t.amount = got;
          const store = sim.nearestStore(p.cellCol, p.cellRow);
          if (!store) {
            p.clearTask();
            break;
          }
          t.toId = store.id;
          t.phase = 'toDest';
          const at = sim.accessCell(store);
          if (!pathTo(sim, p, at.col, at.row)) startDeliver(sim, p);
          break;
        }
        const got = takeStock(sim.stock, t.res, t.amount);
        sim.releaseStock(t.res, t.amount);
        if (got <= 0) {
          abandonTask(sim, p);
          break;
        }
        p.pick(t.res, got);
        t.amount = got;
        const dest = sim.buildings.find((b) => b.id === t.toId);
        if (!dest) {
          startDeliver(sim, p);
          break;
        }
        const at = sim.accessCell(dest);
        t.phase = 'toDest';
        if (!pathTo(sim, p, at.col, at.row)) startDeliver(sim, p);
        break;
      }
      if (!walk(sim, p, dt)) break;
      const dest = sim.buildings.find((b) => b.id === t.toId);
      const load = p.drop();
      if (!dest) {
        sim.deposit(load.res, load.n);
        p.clearTask();
        break;
      }
      if (t.target === 'site') {
        dest.delivered[t.res] = (dest.delivered[t.res] || 0) + load.n;
        dest.incoming[t.res] = Math.max(0, (dest.incoming[t.res] || 0) - t.amount);
      } else if (t.target === 'input') {
        dest.inv[t.res] = (dest.inv[t.res] || 0) + load.n;
        dest.reservedIn[t.res] = Math.max(0, (dest.reservedIn[t.res] || 0) - t.amount);
      } else {
        sim.deposit(load.res, load.n);
      }
      p.clearTask();
      break;
    }
    case 'build': {
      const site = sim.buildings.find((b) => b.id === t.buildingId);
      if (!site || site.built) {
        abandonTask(sim, p);
        break;
      }
      if (t.phase === 'toSite') {
        if (walk(sim, p, dt)) t.phase = 'working';
        break;
      }
      if (!sim.siteReady(site)) {
        abandonTask(sim, p);
        break;
      }
      const rate = cfg.work.buildRate * (sim.mods.build || 1) * cfg.people.workRate * p.skill;
      site.workDone += rate * dt;
      site.active = sim.time;
      doWork(sim, p, rate * dt);
      sim.bufferDirty = true;
      if (site.workDone >= site.work) {
        site.builders = Math.max(0, (site.builders || 0) - 1);
        for (const [res, n] of Object.entries(site.cost)) recordConsumed(sim.econ, res, n);
        sim.finishBuilding(site);
        p.clearTask();
        sim.assignHomes();
      }
      break;
    }
    case 'station': {
      const b = sim.buildings.find((x) => x.id === t.buildingId);
      if (!b || !b.built) {
        p.clearTask();
        break;
      }
      if (t.phase === 'toWork') {
        if (walk(sim, p, dt)) t.phase = 'working';
        break;
      }
      // A worker standing next to a full output bench carries the load in
      // themselves rather than waiting for a hauler to notice.
      const cap = carryLimit(cfg.people, sim.mods);
      if (!p.carrying && outLoad(b) >= cap * 0.75) {
        for (const [res, n] of Object.entries(b.out)) {
          if (n < 1) continue;
          const take = Math.min(Math.floor(n), cap - p.carry.n);
          if (take <= 0) continue;
          b.out[res] = n - take;
          p.pick(res, take);
          break;
        }
        if (p.carrying) {
          startDeliver(sim, p);
          break;
        }
      }
      const job = b.def.job;
      if (job.type === 'craft') {
        if (!sim.craftReady(b) && b.craftProgress <= 0) {
          p.clearTask();
          break;
        }
        const rate = cfg.work.craftRate * (sim.mods.craft || 1) * cfg.people.workRate * p.skill;
        if (b.craftProgress <= 0 && sim.craftReady(b)) {
          for (const [res, n] of Object.entries(job.in)) {
            b.inv[res] -= n;
            recordConsumed(sim.econ, res, n);
          }
          b.craftProgress = 0.0001;
        }
        if (b.craftProgress > 0) {
          b.craftProgress += (rate * dt) / Math.max(0.1, job.time);
          b.active = sim.time;
          doWork(sim, p, rate * dt);
          if (b.craftProgress >= 1) {
            b.craftProgress = 0;
            for (const [res, n] of Object.entries(job.out)) {
              b.out[res] = (b.out[res] || 0) + n;
              recordProduced(sim.econ, res, n);
            }
          }
        }
      } else if (job.type === 'farm') {
        const rate = cfg.work.farmRate * (sim.mods.farm || 1) * cfg.people.workRate * p.skill;
        const fert = sim.farmFertility(b);
        b.craftProgress += rate * dt * fert;
        b.active = sim.time;
        doWork(sim, p, rate * dt);
        while (b.craftProgress >= 1) {
          b.craftProgress -= 1;
          b.out.food = (b.out.food || 0) + 1;
          recordProduced(sim.econ, 'food', 1);
        }
      } else if (job.type === 'research') {
        const rate = cfg.tech.researchPerScholar * (sim.mods.research || 1) * p.skill;
        sim.tech.points += rate * dt;
        b.active = sim.time;
        doWork(sim, p, rate * dt);
      } else if (job.type === 'trade') {
        b.active = sim.time;
        doWork(sim, p, 0.5 * dt);
      }
      p.skill = Math.min(2, p.skill + dt * 0.002);
      // Stations are open ended; step away now and then so the sim can
      // reconsider what this person should be doing.
      t.timer = (t.timer || 0) + dt;
      if (t.timer > 6) p.clearTask();
      break;
    }
    default:
      p.clearTask();
  }
}

// Wages are a market phenomenon here: before the settlement has a market,
// work is subsistence and no coin moves at all.
export function doWork(sim, p, units) {
  if (!sim.hasMarket()) return;
  payWage(sim.econ, sim.cfg.economy, p, units);
}
