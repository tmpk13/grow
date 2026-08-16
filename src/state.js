// Project state: everything the tool can save, load and export.

import { createMaterials, deserializeMaterials, serializeMaterials, invalidateSamplerCache } from './sampler.js';
import { defaultShading } from './shading.js';
import { defaultClassLimits, defaultSpeciesList, makeSpecies } from './species.js';
import { defaultWorldConfig } from './world.js';
import { deepClone } from './util.js';

export const STORAGE_KEY = 'grow.project.v1';

export function createState() {
  return {
    version: 1,
    seed: 20260815,
    materials: createMaterials(),
    shading: defaultShading(),
    species: defaultSpeciesList(),
    classLimits: defaultClassLimits(),
    world: defaultWorldConfig(),
    sim: { speed: 1, running: true, rasterBudget: 12, tickHz: 20 },
  };
}

export function serializeState(state) {
  return {
    version: state.version,
    seed: state.seed,
    materials: serializeMaterials(state.materials),
    shading: deepClone(state.shading),
    species: deepClone(state.species),
    classLimits: deepClone(state.classLimits),
    world: deepClone(state.world),
    sim: deepClone(state.sim),
  };
}

export function deserializeState(data) {
  const fresh = createState();
  const state = {
    version: data.version || 1,
    seed: data.seed ?? fresh.seed,
    materials: data.materials ? deserializeMaterials(data.materials) : fresh.materials,
    shading: { ...fresh.shading, ...(data.shading || {}) },
    // Older projects may miss newer fields; makeSpecies fills the gaps.
    species: (data.species || fresh.species).map((sp) => makeSpecies(sp)),
    classLimits: { ...fresh.classLimits, ...(data.classLimits || {}) },
    world: { ...fresh.world, ...(data.world || {}) },
    sim: { ...fresh.sim, ...(data.sim || {}) },
  };
  invalidateSamplerCache();
  return state;
}

export function saveLocal(state) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(serializeState(state)));
    return true;
  } catch (err) {
    console.warn('save failed', err);
    return false;
  }
}

export function loadLocal() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return deserializeState(JSON.parse(raw));
  } catch (err) {
    console.warn('load failed', err);
    return null;
  }
}

export function clearLocal() {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch (err) {
    console.warn('clear failed', err);
  }
}
