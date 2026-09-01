// The settlement simulation.
//
// It owns a plant world (the same growth sim the editor tunes), a procedural
// terrain under it, and the people who live on top of both. Nothing is handed
// to the people: every wall is built out of materials someone carried there,
// and everything they carry was cut, dug or made somewhere on the map.
//
// The loop each tick is: grow the wilderness, let people act, run production,
// settle the economy, then let research and population catch up.

import { Sim } from '../sim.js';
import { makeRng } from '../rng.js';
import { clamp, clamp01 } from '../util.js';
import { Terrain, CELL } from './terrain.js';
import { BUILDING_BY_ID, BUILDINGS, scaledCost, scaledWork } from './buildings.js';
import { Person, dayFraction, dayNumber, daylight, resetPersonIds } from './people.js';
import { placeName } from './names.js';
import { compositeSettlement, drawCivOverlay, invalidateCivSprites } from './civRender.js';
import { abandonTask, updatePerson } from './tasks.js';
import { findSite, findSiteNear, plan } from './planner.js';
import { RES, RES_IDS, addStock, makeStock, stockBulk, takeStock } from './resources.js';
import {
  logEvent,
  makeEconomy,
  pushHistory,
  recordConsumed,
  rollFlows,
  runCaravan,
  stockTargets,
  updatePrices,
} from './economy.js';
import {
  TECH_BY_ID,
  TECHS,
  available as availableTechs,
  isKnown,
  makeTechState,
  modifiers,
  techCost,
  unlockedBuildings,
} from './tech.js';

export function defaultWorkConfig() {
  return {
    harvestRate: 2.5,
    mineRate: 1.6,
    buildRate: 1.2,
    craftRate: 1,
    farmRate: 0.45,
    // A plant has to be worth the walk before anyone fells it.
    minHarvestMass: 1.5,
    clearYield: 0.5,
    // What a felled plant leaves on the ground rots away over roughly this
    // many days if nobody comes back for it.
    pileLife: 4,
    // Fraction of a full load a hauler is willing to fetch for a workshop.
    restockShare: 1,
    planInterval: 0.5,
  };
}

export function defaultStartConfig() {
  return {
    population: 5,
    supplies: { wood: 30, food: 24, fiber: 12, stone: 6 },
    storehouse: true,
  };
}

const NEIGHBORS = [
  [1, 0], [-1, 0], [0, 1], [0, -1],
  [1, 1], [1, -1], [-1, 1], [-1, -1],
];

export class Settlement {
  constructor(state) {
    this.state = state;
    this.plantSim = new Sim(state, state.civ.world);
    this.world = this.plantSim.world;
    this.buffer = this.plantSim.buffer;
    this.bufferDirty = true;
    this.bg = null;
    this.bgKey = '';
    this.ready = false;
    this.reset(state.civ.seed);
  }

  get cfg() {
    return this.state.civ;
  }

  // ---- lifecycle ---------------------------------------------------------

  reset(seed = this.cfg.seed) {
    const cfg = this.cfg;
    this.rng = makeRng(seed >>> 0);
    this.plantSim.reset(seed);
    this.world = this.plantSim.world;
    const n = this.world.cols * this.world.rows;

    this.plantSim.wildScale = Math.max(0.1, cfg.terrain.wildness || 1);
    this.terrain = new Terrain(this.world, cfg.terrain, seed);
    this.blocked = new Uint8Array(n);
    for (let i = 0; i < n; i++) {
      if (this.terrain.type[i] === CELL.water) this.blocked[i] = 1;
    }
    this.plantSim.blocked = this.blocked;

    this.buildGrid = new Int32Array(n);
    this.traffic = new Float32Array(n);
    this.pathFrom = new Int32Array(n);
    this.pathQueue = new Int32Array(n);

    this.buildings = [];
    this.nextBuildingId = 1;
    // Cut timber and picked food waiting on the ground for somebody to carry
    // it in. A load bigger than one person can lift becomes one of these.
    this.piles = [];
    this.nextPileId = 1;
    this.people = [];
    resetPersonIds();
    this.stock = makeStock(0);
    this.stockReserved = makeStock(0);
    this.econ = makeEconomy(cfg.economy);
    this.tech = makeTechState();
    this.time = 0;
    this.day = 0;
    this.ticks = 0;
    this.planTimer = 0;
    this.births = 0;
    this.deaths = 0;
    this.dead = [];
    this.name = placeName(this.rng);
    this.mods = modifiers(this.tech);
    this.unlocked = unlockedBuildings(this.tech);
    this.buffer = new Uint32Array(this.world.pxW * this.world.pxH);
    this.bg = null;
    this.bgKey = '';
    this.bufferDirty = true;
    this.warmupDone = 0;
    this.ready = false;
    this.terrainVersion = (this.terrainVersion || 0) + 1;
    invalidateCivSprites();
  }

  // Grows the wilderness before the people arrive, then drops the first
  // storehouse and the founding families next to it. Split out from reset so
  // the caller can show a note while it runs.
  bootstrap() {
    if (this.ready) return;
    const cfg = this.cfg;
    const warm = Math.max(0, cfg.terrain.warmup);
    const dt = 1 / Math.max(1, cfg.sim.tickHz);
    for (let t = 0; t < warm; t += dt) this.plantSim.step(dt);
    this.plantSim.processRasterQueue(1e9);
    this.warmupDone = warm;

    const start = cfg.start;
    for (const [id, n] of Object.entries(start.supplies || {})) addStock(this.stock, id, n);

    const spot = this.terrain.findStartCell(this.rng);
    if (start.storehouse) {
      const site = findSiteNear(this, BUILDING_BY_ID.storehouse, spot.col, spot.row, 6);
      if (site) this.placeBuilding('storehouse', site.col, site.row, true);
    }
    this.center = { col: spot.col, row: spot.row };

    const pcfg = cfg.people;
    for (let i = 0; i < start.population; i++) {
      const c = clamp(spot.col + this.rng.int(-2, 2), 0, this.world.cols - 1);
      const r = clamp(spot.row + this.rng.int(-2, 2), 0, this.world.rows - 1);
      const p = new Person({
        col: c,
        row: r,
        age: this.rng.int(pcfg.adultAge + 4, 34),
        rng: this.rng,
      });
      p.adultAge = pcfg.adultAge;
      p.lifespan = this.rng.int(pcfg.lifespanMin, pcfg.lifespanMax);
      p.coin = Math.round(cfg.economy.startCoin / Math.max(1, start.population));
      this.people.push(p);
    }
    logEvent(this.econ, `${this.people.length} people found ${this.name}`, 0);
    this.assignHomes();
    this.assignWorkplaces();
    this.ready = true;
    this.bufferDirty = true;
  }

  // ---- grid helpers ------------------------------------------------------

  idx(c, r) {
    return r * this.world.cols + c;
  }

  inBounds(c, r) {
    return c >= 0 && c < this.world.cols && r >= 0 && r < this.world.rows;
  }

  walkable(c, r) {
    if (!this.inBounds(c, r)) return false;
    const i = this.idx(c, r);
    return this.terrain.type[i] !== CELL.water && this.buildGrid[i] === 0;
  }

  buildingAt(c, r) {
    if (!this.inBounds(c, r)) return null;
    const id = this.buildGrid[this.idx(c, r)];
    return id ? this.buildings.find((b) => b.id === id) || null : null;
  }

  // Free cells touching the building, which is where people stand to use it.
  // A cell whose only neighbors are diagonal is a trap: the path finder does
  // not cut corners, so a spot wedged between a wall and the water can be
  // walkable and still unreachable. Those sort to the back.
  accessCells(b) {
    const cands = [];
    for (let r = b.row - 1; r <= b.row + b.h; r++) {
      for (let c = b.col - 1; c <= b.col + b.w; c++) {
        const inside = c >= b.col && c < b.col + b.w && r >= b.row && r < b.row + b.h;
        if (inside || !this.walkable(c, r)) continue;
        let open = 0;
        if (this.walkable(c + 1, r)) open++;
        if (this.walkable(c - 1, r)) open++;
        if (this.walkable(c, r + 1)) open++;
        if (this.walkable(c, r - 1)) open++;
        cands.push({ col: c, row: r, open });
      }
    }
    // Reachable first, then the near side, so workers stand in front.
    cands.sort((a, z) => (z.open > 0) - (a.open > 0) || z.row - a.row || z.open - a.open);
    if (!cands.length) return [{ col: b.col, row: b.row + b.h }];
    return cands;
  }

  accessCell(b) {
    return this.accessCells(b)[0];
  }

  // Breadth first over the walkable cells. Diagonal steps are allowed but not
  // through the corner between two blocked cells, so nobody walks through a
  // wall joint or across the tip of a lake.
  findPath(sc, sr, tc, tr) {
    if (!this.inBounds(sc, sr) || !this.inBounds(tc, tr)) return null;
    if (sc === tc && sr === tr) return [];
    const cols = this.world.cols;
    const from = this.pathFrom;
    from.fill(-1);
    const queue = this.pathQueue;
    let head = 0;
    let tail = 0;
    const start = this.idx(sc, sr);
    const goal = this.idx(tc, tr);
    queue[tail++] = start;
    from[start] = start;
    let found = false;
    while (head < tail) {
      const cur = queue[head++];
      if (cur === goal) {
        found = true;
        break;
      }
      const cc = cur % cols;
      const cr = (cur / cols) | 0;
      for (const [dx, dy] of NEIGHBORS) {
        const nc = cc + dx;
        const nr = cr + dy;
        if (!this.inBounds(nc, nr)) continue;
        const ni = this.idx(nc, nr);
        if (from[ni] !== -1) continue;
        if (ni !== goal && !this.walkable(nc, nr)) continue;
        if (dx !== 0 && dy !== 0 && !(this.walkable(cc + dx, cr) && this.walkable(cc, cr + dy))) continue;
        from[ni] = cur;
        queue[tail++] = ni;
      }
    }
    if (!found) return null;
    const path = [];
    let cur = goal;
    while (cur !== start) {
      path.push({ col: cur % cols, row: (cur / cols) | 0 });
      cur = from[cur];
    }
    path.reverse();
    return path;
  }

  // ---- buildings ---------------------------------------------------------

  storeCapacity() {
    let cap = 0;
    for (const b of this.buildings) {
      if (b.built && b.def.storage) cap += b.def.storage;
    }
    return cap;
  }

  storeSpace() {
    return Math.max(0, this.storeCapacity() - stockBulk(this.stock));
  }

  nearestStore(col, row) {
    let best = null;
    let bestD = Infinity;
    for (const b of this.buildings) {
      if (!b.built || !b.def.isStore) continue;
      const d = (b.col - col) ** 2 + (b.row - row) ** 2;
      if (d < bestD) {
        best = b;
        bestD = d;
      }
    }
    return best;
  }

  hasMarket() {
    return this.buildings.some((b) => b.built && b.def.isMarket);
  }

  countBuilt(typeId) {
    let n = 0;
    for (const b of this.buildings) if (b.type === typeId && b.built) n++;
    return n;
  }

  countAll(typeId) {
    let n = 0;
    for (const b of this.buildings) if (b.type === typeId) n++;
    return n;
  }

  get sites() {
    return this.buildings.filter((b) => !b.built);
  }

  housingCapacity() {
    let cap = 0;
    for (const b of this.buildings) if (b.built && b.def.housing) cap += b.def.housing;
    return cap;
  }

  workSlots(typeId) {
    let open = 0;
    for (const b of this.buildings) {
      if (b.built && b.type === typeId) open += (b.def.slots || 0) - b.workers.length;
    }
    return open;
  }

  placeBuilding(typeId, col, row, instant = false) {
    const def = BUILDING_BY_ID[typeId];
    if (!def) return null;
    const cost = scaledCost(def, this.cfg.build);
    const b = {
      id: this.nextBuildingId++,
      type: typeId,
      def,
      col,
      row,
      w: def.w,
      h: def.h,
      built: false,
      cost,
      delivered: {},
      incoming: {},
      work: scaledWork(def, this.cfg.build),
      workDone: 0,
      inv: {},
      out: {},
      reservedIn: {},
      reservedOut: {},
      workers: [],
      craftProgress: 0,
      seed: this.rng.seed(),
      active: 0,
      founded: this.time,
    };
    this.buildings.push(b);
    for (let r = row; r < row + def.h; r++) {
      for (let c = col; c < col + def.w; c++) {
        const i = this.idx(c, r);
        this.buildGrid[i] = b.id;
        this.blocked[i] = 1;
      }
    }
    this.clearPlantsUnder(b);
    this.groundDirty = true;
    if (instant) {
      for (const [id, n] of Object.entries(cost)) b.delivered[id] = n;
      b.workDone = b.work;
      this.finishBuilding(b);
    }
    this.bufferDirty = true;
    return b;
  }

  // Ground is cleared before anything is raised on it; half the timber of
  // whatever stood there goes into the store.
  clearPlantsUnder(b) {
    const pad = 0;
    for (let i = this.plantSim.plants.length - 1; i >= 0; i--) {
      const p = this.plantSim.plants[i];
      if (
        p.col >= b.col - pad && p.col < b.col + b.w + pad &&
        p.row >= b.row - pad && p.row < b.row + b.h + pad
      ) {
        const mass = this.plantMass(p);
        const share = this.cfg.work.clearYield;
        if (p.species.sizeClass === 'tree' || p.species.sizeClass === 'shrub') {
          this.deposit('wood', Math.round(mass * share));
        } else {
          this.deposit('fiber', Math.round(mass * share * 0.5));
        }
        this.plantSim.removePlantAt(i);
      }
    }
  }

  finishBuilding(b) {
    b.built = true;
    b.workDone = b.work;
    this.groundDirty = true;
    logEvent(this.econ, `${b.def.label} finished`, this.day);
    this.assignWorkplaces();
    this.bufferDirty = true;
  }

  removeBuilding(b) {
    const at = this.buildings.indexOf(b);
    if (at === -1) return;
    this.buildings.splice(at, 1);
    for (let r = b.row; r < b.row + b.h; r++) {
      for (let c = b.col; c < b.col + b.w; c++) {
        const i = this.idx(c, r);
        this.buildGrid[i] = 0;
        if (this.terrain.type[i] !== CELL.water) this.blocked[i] = 0;
      }
    }
    for (const p of this.people) {
      if (p.work === b.id) p.work = 0;
      if (p.home === b.id) p.home = 0;
      if (p.task && (p.task.buildingId === b.id || p.task.toId === b.id || p.task.fromId === b.id)) {
        abandonTask(this, p);
      }
    }
    this.bufferDirty = true;
  }

  // ---- piles -------------------------------------------------------------

  addPile(col, row, res, n) {
    if (n <= 0) return null;
    // The spot is resolved before the merge, or a load dropped on a blocked
    // cell would start a new pile on every single delivery.
    const spot = this.freeCellNear(col, row) || { col, row };
    const existing = this.piles.find((q) => q.col === spot.col && q.row === spot.row && q.res === res);
    if (existing) {
      existing.n += n;
      this.bufferDirty = true;
      return existing;
    }
    const pile = {
      id: this.nextPileId++,
      col: spot.col,
      row: spot.row,
      res,
      n,
      claimedBy: 0,
      seed: this.rng.seed(),
    };
    this.piles.push(pile);
    this.bufferDirty = true;
    return pile;
  }

  takePile(pile, n) {
    const got = Math.min(pile.n, n);
    pile.n -= got;
    if (pile.n <= 0.01) this.removePile(pile);
    this.bufferDirty = true;
    return got;
  }

  removePile(pile) {
    const at = this.piles.indexOf(pile);
    if (at !== -1) this.piles.splice(at, 1);
  }

  pilesTick(dt) {
    const life = Math.max(0.2, this.cfg.work.pileLife) * this.cfg.people.dayLength;
    const keep = Math.exp(-dt / life);
    for (let i = this.piles.length - 1; i >= 0; i--) {
      const pile = this.piles[i];
      pile.n *= keep;
      if (pile.n < 0.5) this.piles.splice(i, 1);
    }
  }

  // ---- stock -------------------------------------------------------------

  // Anything the store has no room for is left outside it, where it slowly
  // rots. A settlement that keeps gathering past its storage loses the surplus,
  // which is what makes another storehouse worth building.
  // Nobody carries home what the settlement is already drowning in.
  wanted(res) {
    const targets = stockTargets(this.cfg.economy, this.people.length);
    const limit = (targets[res] || 10) * (this.cfg.economy.hoardLimit || 2.5);
    return (this.stock[res] || 0) < limit;
  }

  deposit(res, n, col = null, row = null) {
    if (n <= 0) return 0;
    if (!this.wanted(res)) {
      const at = col === null ? this.center || { col: 0, row: 0 } : { col, row };
      this.addPile(at.col, at.row, res, n);
      return 0;
    }
    const space = this.storeSpace();
    const bulk = RES[res] ? RES[res].bulk || 1 : 1;
    const room = Math.floor(space / bulk);
    const put = Math.max(0, Math.min(n, room));
    if (put > 0) addStock(this.stock, res, put);
    const over = n - put;
    if (over > 0) {
      const at = col === null ? this.center || { col: 0, row: 0 } : { col, row };
      this.addPile(at.col, at.row, res, over);
    }
    return put;
  }

  availableStock(res) {
    return Math.max(0, (this.stock[res] || 0) - (this.stockReserved[res] || 0));
  }

  reserveStock(res, n) {
    this.stockReserved[res] = (this.stockReserved[res] || 0) + n;
  }

  releaseStock(res, n) {
    this.stockReserved[res] = Math.max(0, (this.stockReserved[res] || 0) - n);
  }

  // ---- assignment --------------------------------------------------------

  assignHomes() {
    const homes = this.buildings.filter((b) => b.built && b.def.housing);
    const used = new Map();
    for (const p of this.people) {
      if (!p.home) continue;
      const home = homes.find((h) => h.id === p.home);
      if (!home) {
        p.home = 0;
        continue;
      }
      used.set(home.id, (used.get(home.id) || 0) + 1);
    }
    for (const p of this.people) {
      if (p.home) continue;
      for (const h of homes) {
        const n = used.get(h.id) || 0;
        if (n < h.def.housing) {
          p.home = h.id;
          used.set(h.id, n + 1);
          break;
        }
      }
    }
  }

  // Labor is reallocated from scratch every day: workplaces are ranked by what
  // the settlement is short of and filled from the adults, with a strong
  // preference for keeping people where they already work. Without the
  // reshuffle a settlement staffs its quarries while it starves, because a
  // worker already in a job is never reconsidered.
  assignWorkplaces() {
    const pcfg = this.cfg.people;
    const adults = this.people.filter((p) => p.alive && p.adult);
    const reserve = Math.max(1, Math.round(adults.length * clamp01(pcfg.laborerShare)));
    const employable = Math.max(0, adults.length - reserve);
    const previous = new Map(adults.map((p) => [p.id, p.work]));
    for (const b of this.buildings) b.workers = [];
    for (const p of adults) {
      p.work = 0;
      p.profession = 'laborer';
    }

    const openings = this.buildings
      .filter((b) => b.built && (b.def.slots || 0) > 0)
      .map((b) => ({ b, priority: this.jobPriority(b) }))
      .sort((x, y) => y.priority - x.priority);
    const free = adults.slice();
    let employed = 0;

    for (const { b, priority } of openings) {
      if (priority <= -0.6) continue;
      while (b.workers.length < b.def.slots && employed < employable && free.length) {
        let bestAt = 0;
        let bestScore = -Infinity;
        for (let i = 0; i < free.length; i++) {
          const p = free[i];
          const dist = Math.hypot(p.x - b.col, p.y - b.row);
          const sticky = previous.get(p.id) === b.id ? 12 : 0;
          const score = sticky - dist * 0.2;
          if (score > bestScore) {
            bestScore = score;
            bestAt = i;
          }
        }
        const p = free.splice(bestAt, 1)[0];
        p.work = b.id;
        p.profession = professionFor(b.def);
        b.workers.push(p.id);
        employed++;
      }
    }

    for (const p of this.people) {
      if (!p.adult) p.profession = 'child';
      else if (previous.get(p.id) !== p.work && p.task && p.task.kind !== 'sleep') abandonTask(this, p);
    }
  }

  // How badly an open workplace wants filling, given what the store is short
  // of. Food first, then whatever the build planner is waiting on.
  jobPriority(b) {
    const targets = stockTargets(this.cfg.economy, this.people.length);
    const job = b.def.job;
    if (!job) return 0;
    if (job.type === 'harvest' || job.type === 'mine' || job.type === 'farm') {
      const yields = Object.keys(job.yields || { food: 1 });
      // A camp with nothing left to cut within reach is not worth staffing,
      // however badly the store wants what it used to produce. This is what
      // pushes a settlement off foraging and onto farming.
      let supply = 1;
      if (job.type === 'harvest') {
        const mass = this.harvestableMass(b.col, b.row, (b.def.radius || 12) * 1.5, job.classes);
        supply = clamp01(mass / Math.max(1, (b.def.slots || 1) * 40));
      } else if (job.type === 'mine') {
        const dep = this.terrain.findDeposit(job.deposit, b.col, b.row, b.def.radius || 12);
        supply = dep ? 1 : 0;
      }
      // The scarcest thing a job produces sets its priority. Summing instead
      // lets a byproduct nobody needs cancel out a shortage of the main one,
      // which is how a settlement ends up with no wood and eight foragers.
      let need = -1;
      for (const res of yields) {
        const short = clamp((targets[res] - (this.stock[res] || 0)) / Math.max(1, targets[res]), -1, 1);
        need = Math.max(need, short * this.demandFor(res));
      }
      // Farming is the reliable half of the food supply, so it outranks a
      // forager camp when both are hungry for hands.
      const settled = job.type === 'farm' ? 0.3 : 0;
      if (yields.includes('food')) return Math.max(0.3 * supply, (need + 0.6 + settled) * supply);
      return need * supply;
    }
    if (job.type === 'craft') {
      let inputs = 1;
      for (const [res, n] of Object.entries(job.in)) {
        inputs = Math.min(inputs, (this.stock[res] || 0) / Math.max(1, n * 4));
      }
      let want = -1;
      for (const res of Object.keys(job.out)) {
        const short = clamp((targets[res] - (this.stock[res] || 0)) / Math.max(1, targets[res]), -1, 1);
        want = Math.max(want, short * this.demandFor(res));
      }
      return want * inputs - 0.2;
    }
    if (job.type === 'research') return 0.35;
    if (job.type === 'trade') return 0.3;
    return 0;
  }

  // Standing biomass a camp could still cut, used both to decide whether it is
  // worth staffing and to place a new camp.
  harvestableMass(col, row, radius, classes) {
    let total = 0;
    const min = this.cfg.work.minHarvestMass;
    for (const plant of this.plantSim.plants) {
      if (!classes.includes(plant.species.sizeClass)) continue;
      const dx = plant.col - col;
      const dy = plant.row - row;
      if (dx * dx + dy * dy > radius * radius) continue;
      const mass = this.plantMass(plant);
      if (mass >= min) total += mass;
    }
    return total;
  }

  // How much the settlement actually wants a resource, beyond the abstract
  // stock target: something nothing consumes and nothing is built out of is
  // barely worth making. Charcoal only matters once a kiln stands.
  demandFor(res) {
    if (res === 'food' || res === 'wood') return 1;
    let demand = 0.15;
    for (const b of this.buildings) {
      if (!b.built || !b.def.job || b.def.job.type !== 'craft') continue;
      if (b.def.job.in[res]) return 1;
    }
    for (const def of BUILDINGS) {
      if (!def.base && !this.unlocked.has(def.id)) continue;
      if ((def.cost || {})[res]) demand = Math.max(demand, 0.85);
      // A recipe that is unlocked but not yet standing still counts, or the
      // settlement would never make the charcoal it needs to build the kiln
      // that would have created the demand for charcoal.
      if (def.job && def.job.in && def.job.in[res]) demand = Math.max(demand, 0.7);
    }
    return demand;
  }

  // ---- main step ---------------------------------------------------------

  step(dt) {
    if (!this.ready) return;
    const cfg = this.cfg;
    this.time += dt;
    this.ticks++;
    this.plantSim.step(dt);
    // A plant that was re-drawn or removed changes the shadows on the ground.
    if (this.plantSim.bufferDirty) {
      this.plantSim.bufferDirty = false;
      this.groundDirty = true;
    }

    this.planTimer -= dt;
    if (this.planTimer <= 0) {
      this.planTimer = Math.max(0.1, cfg.work.planInterval);
      plan(this);
    }

    for (let i = this.people.length - 1; i >= 0; i--) {
      const p = this.people[i];
      updatePerson(this, p, dt);
      if (!p.alive) {
        this.buryPerson(i);
      }
    }

    this.productionTick();
    this.pilesTick(dt);
    this.economyTick(dt);
    this.researchTick(dt);

    const day = dayNumber(this.time, cfg.people);
    if (day !== this.day) {
      this.day = day;
      this.dayTick();
    }

    // Footpaths fade unless they keep being walked. The whole grid is swept at
    // most once a simulated second, with the elapsed time compounded into the
    // decay, rather than every tick.
    this.trafficTimer = (this.trafficTimer || 0) + dt;
    if (this.trafficTimer >= 1) {
      const decay = Math.exp(-this.trafficTimer * 0.02);
      this.trafficTimer = 0;
      for (let i = 0; i < this.traffic.length; i++) {
        if (this.traffic[i] > 0.002) this.traffic[i] *= decay;
        else this.traffic[i] = 0;
      }
    }
    this.bufferDirty = true;
  }

  dayTick() {
    const cfg = this.cfg;
    rollFlows(this.econ);
    this.assignWorkplaces();
    this.populationTick();
    this.decayFood();
    pushHistory(this.econ, cfg.economy, {
      day: this.day,
      pop: this.people.length,
      coin: Math.round(this.econ.coin),
      food: Math.round(this.stock.food || 0),
      wood: Math.round(this.stock.wood || 0),
      research: Math.round(this.tech.points),
      buildings: this.buildings.filter((b) => b.built).length,
      happiness: this.averageHappiness(),
    });
  }

  decayFood() {
    const keep = this.buildings.some((b) => b.built && b.def.keepsFood) ? 0.35 : 1;
    for (const id of RES_IDS) {
      const rate = RES[id].decay * keep;
      if (!rate) continue;
      const lost = (this.stock[id] || 0) * rate * this.cfg.people.dayLength;
      if (lost >= 0.5) {
        takeStock(this.stock, id, Math.floor(lost));
        recordConsumed(this.econ, id, Math.floor(lost));
      }
    }
  }

  // ---- people ------------------------------------------------------------

  buryPerson(index) {
    const p = this.people[index];
    if (p.carry.n > 0) this.deposit(p.carry.res, p.carry.n);
    abandonTask(this, p);
    this.people.splice(index, 1);
    this.deaths++;
    this.dead.push({ name: p.name, age: Math.floor(p.age), cause: p.cause, day: this.day });
    if (this.dead.length > 30) this.dead.shift();
    const home = this.buildings.find((b) => b.id === p.home);
    if (home) this.assignHomes();
    logEvent(this.econ, `${p.name} died of ${p.cause} at ${Math.floor(p.age)}`, this.day);
    this.assignWorkplaces();
  }

  populationTick() {
    const cfg = this.cfg;
    const pcfg = cfg.people;
    const capacity = this.housingCapacity();
    const pop = this.people.length;
    if (pop >= capacity) return;
    // Days of food per person in store, not the size of the heap: a growing
    // settlement has to keep pace with itself to keep growing.
    const fed = clamp01((this.stock.food || 0) / Math.max(1, pop * pcfg.mealSize * 3));
    const couples = this.people.filter((p) => p.alive && p.adult && p.age < pcfg.fertileUntil && p.home).length / 2;
    // Births per couple per day, thinned by how well fed and housed they are.
    const rate = pcfg.birthRate * (this.mods.comfort || 1) * fed * couples;
    let births = Math.floor(rate);
    if (this.rng.chance(rate - births)) births++;
    for (let i = 0; i < births && this.people.length < capacity; i++) {
      const parent = this.people.find((p) => p.adult && p.home);
      if (!parent) break;
      const child = new Person({
        col: parent.cellCol,
        row: parent.cellRow,
        age: 0,
        rng: this.rng,
      });
      child.adultAge = pcfg.adultAge;
      child.lifespan = this.rng.int(pcfg.lifespanMin, pcfg.lifespanMax);
      child.home = parent.home;
      child.born = this.day;
      this.people.push(child);
      this.births++;
      logEvent(this.econ, `${child.name} was born`, this.day);
    }
    if (births) this.assignHomes();
  }

  wellCoverage(p) {
    let best = 0;
    for (const b of this.buildings) {
      if (!b.built || !b.def.health) continue;
      const d = Math.hypot(b.col - p.x, b.row - p.y);
      if (d <= (b.def.radius || 10)) best = Math.max(best, b.def.health);
    }
    return best;
  }

  averageHappiness() {
    if (!this.people.length) return 0;
    let sum = 0;
    for (const p of this.people) sum += p.happiness;
    return sum / this.people.length;
  }

  // ---- tasks -------------------------------------------------------------

  plantMass(plant) {
    const cellPx = this.world.cellPx;
    return (plant.heightPx + plant.radiusPx * 2) / cellPx;
  }

  siteReady(site) {
    for (const [res, n] of Object.entries(site.cost)) {
      if ((site.delivered[res] || 0) < n) return false;
    }
    return site.workDone < site.work;
  }

  craftReady(b) {
    for (const [res, n] of Object.entries(b.def.job.in)) {
      if ((b.inv[res] || 0) < n) return false;
    }
    return true;
  }

  freeCellNear(col, row) {
    if (this.walkable(col, row)) return { col, row };
    for (let radius = 1; radius <= 3; radius++) {
      for (let r = row - radius; r <= row + radius; r++) {
        for (let c = col - radius; c <= col + radius; c++) {
          if (this.walkable(c, r)) return { col: c, row: r };
        }
      }
    }
    return null;
  }

  // Workers stand around their building rather than inside it, spread along
  // the free cells next to it so a crowded workshop still reads clearly.
  workSpot(b, p) {
    const at = this.accessCell(b);
    const slot = b.workers.indexOf(p.id);
    if (slot <= 0) return at;
    const offsets = [[0, 0], [1, 0], [-1, 0], [0, 1], [1, 1], [-1, 1]];
    const [dx, dy] = offsets[slot % offsets.length];
    const c = at.col + dx;
    const r = at.row + dy;
    return this.walkable(c, r) ? { col: c, row: r } : at;
  }

  farmFertility(b) {
    const rad = b.def.fields || 2;
    let sum = 0;
    let n = 0;
    for (let r = b.row - rad; r <= b.row + b.h + rad; r++) {
      for (let c = b.col - rad; c <= b.col + b.w + rad; c++) {
        if (!this.inBounds(c, r)) continue;
        sum += this.terrain.fertility(c, r);
        n++;
      }
    }
    return n ? clamp(0.25 + (sum / n) * 1.5, 0.1, 2.5) : 0.4;
  }

  // ---- production, economy, research -------------------------------------

  // Workshops with nobody in them slowly lose their half made goods, which
  // keeps abandoned buildings from holding stock hostage.
  productionTick() {
    for (const b of this.buildings) {
      if (!b.built || !b.def.job) continue;
      if (b.def.job.type === 'craft' && b.workers.length === 0 && b.craftProgress > 0) {
        b.craftProgress = Math.max(0, b.craftProgress - 0.02);
      }
    }
  }

  economyTick(dt) {
    const cfg = this.cfg.economy;
    updatePrices(this.econ, cfg, this.stock, this.people.length, dt);
    if (this.hasMarket()) {
      this.econ.tradeTimer += dt;
      if (this.econ.tradeTimer >= cfg.tradeInterval) {
        this.econ.tradeTimer = 0;
        runCaravan(this.econ, cfg, this.stock, this.people.length, this.mods, this.rng, this.day);
      }
    }
  }

  researchTick(dt) {
    const cfg = this.cfg.tech;
    this.tech.points += this.people.length * cfg.insightPerPerson * (this.mods.research || 1) * dt;
    const targetId = this.tech.target && !isKnown(this.tech, this.tech.target) ? this.tech.target : null;
    let target = targetId ? TECH_BY_ID[targetId] : null;
    if (target && !target.requires.every((r) => isKnown(this.tech, r))) target = null;
    if (!target && cfg.autoResearch) target = this.pickResearch();
    if (!target) return;
    const cost = techCost(target, cfg);
    if (this.tech.points >= cost) {
      this.tech.points -= cost;
      this.tech.spent += cost;
      this.tech.known.push(target.id);
      this.tech.log.push({ id: target.id, day: this.day });
      this.mods = modifiers(this.tech);
      this.unlocked = unlockedBuildings(this.tech);
      if (this.tech.target === target.id) this.tech.target = null;
      logEvent(this.econ, `learned ${target.label}`, this.day);
    }
  }

  // Cheapest reachable tech, nudged toward whatever unlocks something the
  // settlement is currently short of.
  pickResearch() {
    const cfg = this.cfg.tech;
    const options = availableTechs(this.tech);
    if (!options.length) return null;
    const targets = stockTargets(this.cfg.economy, this.people.length);
    let best = null;
    let bestScore = -Infinity;
    for (const t of options) {
      let score = -techCost(t, cfg) / 100;
      let need = 0;
      for (const bid of t.unlocks) {
        const def = BUILDING_BY_ID[bid];
        if (!def) continue;
        if (def.housing && this.housingCapacity() < this.people.length + this.cfg.build.housingSlack) need += 1;
        const job = def.job;
        if (!job) continue;
        const out = job.out || job.yields || {};
        for (const res of Object.keys(out)) {
          const short = clamp((targets[res] - (this.stock[res] || 0)) / Math.max(1, targets[res]), 0, 1);
          // A hungry settlement should be reaching for agriculture, not for
          // whatever happens to be cheapest.
          need += res === 'food' ? short * 3 : short;
        }
      }
      score += need * (1 + cfg.needBias);
      if (score > bestScore) {
        bestScore = score;
        best = t;
      }
    }
    return best;
  }

  // ---- the build planner -------------------------------------------------

  // Queued by hand from the build panel: same placement rules, no planner.
  queueBuilding(typeId) {
    const def = BUILDING_BY_ID[typeId];
    if (!def) return null;
    const site = findSite(this, def);
    if (!site) return null;
    return this.placeBuilding(typeId, site.col, site.row);
  }

  // ---- reporting ---------------------------------------------------------

  stats() {
    const professions = {};
    let children = 0;
    for (const p of this.people) {
      professions[p.profession] = (professions[p.profession] || 0) + 1;
      if (!p.adult) children++;
    }
    return {
      name: this.name,
      day: this.day,
      dayFraction: dayFraction(this.time, this.cfg.people),
      daylight: daylight(this.time, this.cfg.people),
      population: this.people.length,
      children,
      professions,
      housing: this.housingCapacity(),
      buildings: this.buildings.filter((b) => b.built).length,
      sites: this.sites.length,
      storage: this.storeCapacity(),
      bulk: stockBulk(this.stock),
      coin: this.econ.coin,
      research: this.tech.points,
      known: this.tech.known.length,
      techs: TECHS.length,
      births: this.births,
      deaths: this.deaths,
      happiness: this.averageHappiness(),
      time: this.time,
      ticks: this.ticks,
    };
  }

  // ---- drawing -----------------------------------------------------------

  get daylight() {
    return daylight(this.time, this.cfg.people);
  }

  // Windows are lit once it is dark enough to want them.
  get nightLights() {
    return this.daylight < 0.4;
  }

  composite() {
    return compositeSettlement(this);
  }

  overlay(ctx, viewport) {
    drawCivOverlay(this, ctx, viewport);
  }

  // The sampling boxes or the cell size changed, so every generated sprite has
  // to be built again.
  invalidateSprites() {
    invalidateCivSprites();
    this.bg = null;
    this.groundDirty = true;
    this.bufferDirty = true;
  }

  // Compatibility with the plant sim view: the settlement is rasterized by the
  // same viewport, so it answers the same two questions.
  processRasterQueue(budget) {
    return this.plantSim.processRasterQueue(budget);
  }

  markAllDirty() {
    this.plantSim.markAllDirty();
    this.bufferDirty = true;
  }
}

function professionFor(def) {
  if (!def.job) return 'laborer';
  switch (def.job.type) {
    case 'harvest':
      return def.job.yields && def.job.yields.wood ? 'woodcutter' : 'forager';
    case 'mine':
      return 'miner';
    case 'farm':
      return 'farmer';
    case 'craft':
      return 'crafter';
    case 'research':
      return 'scholar';
    case 'trade':
      return 'trader';
    default:
      return 'laborer';
  }
}

