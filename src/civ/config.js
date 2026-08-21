// Every knob the settlement runs on, in one place.
//
// The sections mirror the panels: land (map and terrain), people, work rates,
// building and planning, economy, technology. Nothing in the simulation reads
// a constant that is not reachable from here.

import { defaultWorldConfig } from '../world.js';
import { defaultTerrainConfig } from './terrain.js';
import { defaultPeopleConfig } from './people.js';
import { defaultEconomyConfig } from './economy.js';
import { defaultTechConfig } from './tech.js';
import { defaultBuildConfig } from './buildings.js';
import { defaultStartConfig, defaultWorkConfig } from './settlement.js';

export function defaultCivWorld() {
  return {
    ...defaultWorldConfig(),
    cols: 88,
    rows: 36,
    cellPx: 8,
    depthPx: 5,
    skyPx: 110,
    depthFade: 0.14,
  };
}

export function defaultCivConfig() {
  return {
    seed: 77104,
    world: defaultCivWorld(),
    terrain: defaultTerrainConfig(),
    people: defaultPeopleConfig(),
    work: defaultWorkConfig(),
    build: defaultBuildConfig(),
    economy: defaultEconomyConfig(),
    tech: defaultTechConfig(),
    start: defaultStartConfig(),
    sim: { speed: 1, running: true, tickHz: 20, rasterBudget: 12 },
    view: {
      dayNight: true,
      paths: true,
      deposits: true,
      people: true,
      labels: false,
      smoke: true,
      waterTop: '#2b4f63',
      waterDeep: '#16303f',
      pathColor: '#6b5a44',
    },
  };
}

// Merges a loaded project over the defaults section by section, so a project
// saved before a parameter existed still gets a value for it.
export function mergeCivConfig(data) {
  const fresh = defaultCivConfig();
  if (!data) return fresh;
  const merged = { ...fresh };
  for (const key of Object.keys(fresh)) {
    const incoming = data[key];
    if (incoming && typeof incoming === 'object' && !Array.isArray(incoming)) {
      merged[key] = { ...fresh[key], ...incoming };
      // Deposits are a nested table of their own.
      if (key === 'terrain' && incoming.deposits) {
        merged.terrain.deposits = { ...fresh.terrain.deposits };
        for (const [kind, cfg] of Object.entries(incoming.deposits)) {
          merged.terrain.deposits[kind] = { ...(fresh.terrain.deposits[kind] || {}), ...cfg };
        }
      }
      if (key === 'build' && incoming.weights) {
        merged.build.weights = { ...fresh.build.weights, ...incoming.weights };
      }
      if (key === 'start' && incoming.supplies) {
        merged.start.supplies = { ...fresh.start.supplies, ...incoming.supplies };
      }
    } else if (incoming !== undefined) {
      merged[key] = incoming;
    }
  }
  return merged;
}
