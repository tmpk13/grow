// People.
//
// A person is data plus two mechanical pieces that do not need the rest of the
// world: needs that drift over time, and movement along a path of cells. Every
// decision about what to do next is made by the settlement, which is the only
// thing that can see jobs, buildings and stock.

import { clamp, clamp01 } from '../util.js';
import { familyName, personName } from './names.js';

export const PROFESSIONS = {
  laborer: 'Laborer',
  woodcutter: 'Woodcutter',
  forager: 'Forager',
  miner: 'Miner',
  farmer: 'Farmer',
  crafter: 'Crafter',
  scholar: 'Scholar',
  trader: 'Trader',
  child: 'Child',
};

export function defaultPeopleConfig() {
  return {
    startPopulation: 5,
    walkSpeed: 2.4,
    carryCapacity: 12,
    workRate: 1,
    // Needs are expressed per simulated second; a day is dayLength seconds, so
    // a hunger rate of 0.008 means half a day from a full meal to hungry.
    dayLength: 120,
    workStart: 0.2,
    workEnd: 0.8,
    hungerRate: 0.008,
    eatAt: 0.55,
    mealSize: 2,
    tireRate: 0.006,
    sleepRate: 0.2,
    starveDamage: 0.006,
    healRate: 0.01,
    birthRate: 0.12,
    adultAge: 12,
    yearsPerDay: 0.45,
    lifespanMin: 62,
    lifespanMax: 92,
    fertileUntil: 46,
    sicknessRate: 0.008,
    // Fraction of adults kept free of a workplace to haul and build.
    laborerShare: 0.3,
    roadSpeedBonus: 0.35,
  };
}

let nextPersonId = 1;

export function resetPersonIds() {
  nextPersonId = 1;
}

export class Person {
  constructor({ col, row, age, rng }) {
    this.id = nextPersonId++;
    this.rng = rng;
    this.seed = rng.seed();
    this.name = `${personName(rng)} ${familyName(rng)}`;
    this.col = col;
    this.row = row;
    // Fractional position on the ground plane, in cells.
    this.x = col + 0.5;
    this.y = row + 0.5;
    this.age = age;
    this.lifespan = 0;
    this.alive = true;
    this.cause = null;
    this.hunger = rng.range(0, 0.3);
    this.energy = rng.range(0.7, 1);
    this.health = 1;
    this.happiness = 0.6;
    this.coin = 0;
    this.home = 0;
    this.work = 0;
    this.profession = 'laborer';
    this.carry = { res: null, n: 0 };
    this.task = null;
    this.path = null;
    this.pathAt = 0;
    this.sleeping = false;
    this.facing = 1;
    this.bob = rng.range(0, Math.PI * 2);
    this.skill = 1;
    this.wage = 0;
    this.born = 0;
  }

  get adult() {
    return this.age >= this.adultAge;
  }

  get carrying() {
    return this.carry.n > 0;
  }

  get cellCol() {
    return Math.floor(this.x);
  }

  get cellRow() {
    return Math.floor(this.y);
  }

  setPath(path) {
    this.path = path && path.length ? path : null;
    this.pathAt = 0;
  }

  clearTask() {
    this.task = null;
    this.path = null;
    this.pathAt = 0;
  }

  // Advances along the current path. Returns true once the end is reached,
  // which is also the answer when there is no path at all.
  moveAlong(dt, speed) {
    if (!this.path) return true;
    let budget = speed * dt;
    while (budget > 0 && this.path) {
      const node = this.path[this.pathAt];
      const tx = node.col + 0.5;
      const ty = node.row + 0.5;
      const dx = tx - this.x;
      const dy = ty - this.y;
      const d = Math.hypot(dx, dy);
      if (d <= budget || d < 1e-4) {
        this.x = tx;
        this.y = ty;
        budget -= d;
        this.pathAt++;
        if (this.pathAt >= this.path.length) {
          this.path = null;
          this.pathAt = 0;
          return true;
        }
      } else {
        this.x += (dx / d) * budget;
        this.y += (dy / d) * budget;
        if (Math.abs(dx) > 0.01) this.facing = dx > 0 ? 1 : -1;
        budget = 0;
      }
    }
    this.bob += speed * dt * 6;
    return !this.path;
  }

  // Needs drift whether or not the person has anything to do.
  tickNeeds(dt, cfg, working) {
    this.hunger = clamp01(this.hunger + cfg.hungerRate * dt * (working ? 1.3 : 1));
    if (this.sleeping) {
      this.energy = clamp01(this.energy + cfg.sleepRate * dt);
    } else {
      this.energy = clamp01(this.energy - cfg.tireRate * dt * (working ? 1.6 : 0.6));
    }
    if (this.hunger >= 1) {
      this.health = clamp01(this.health - cfg.starveDamage * dt);
    } else if (this.hunger < 0.6) {
      this.health = clamp01(this.health + cfg.healRate * dt);
    }
    const comfort = (this.home ? 0.5 : 0) + (1 - this.hunger) * 0.3 + this.energy * 0.2;
    this.happiness = clamp01(this.happiness + (comfort - this.happiness) * clamp(dt * 0.1, 0, 1));
  }

  eat(mealSize) {
    this.hunger = clamp01(this.hunger - mealSize * 0.4);
  }

  pick(res, n) {
    if (this.carry.res && this.carry.res !== res) return 0;
    this.carry.res = res;
    this.carry.n += n;
    return n;
  }

  drop() {
    const out = { res: this.carry.res, n: this.carry.n };
    this.carry.res = null;
    this.carry.n = 0;
    return out;
  }
}

export function carryLimit(cfg, mods) {
  return Math.max(1, Math.round(cfg.carryCapacity * (mods.carry || 1)));
}

// Day fraction in [0,1): 0 is midnight, 0.5 midday.
export function dayFraction(time, cfg) {
  const len = Math.max(1, cfg.dayLength);
  return (time % len) / len;
}

export function dayNumber(time, cfg) {
  return Math.floor(time / Math.max(1, cfg.dayLength));
}

export function isWorkTime(time, cfg) {
  const f = dayFraction(time, cfg);
  return f >= cfg.workStart && f < cfg.workEnd;
}

// Daylight in [0,1], used for the sky tint and for how well people work.
export function daylight(time, cfg) {
  const f = dayFraction(time, cfg);
  return clamp01(Math.sin(Math.PI * clamp01((f - 0.12) / 0.76)) * 1.15);
}
