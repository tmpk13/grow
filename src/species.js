// Species definitions: what a plant is, how fast it appears, how it grows and
// which sampling boxes its materials come from.
//
// Every plant belongs to a size class. The class owns the occupancy layer and
// the hard ceilings for footprint and height; a species sets its own limits
// within those ceilings (the effective value is the smaller of the two).

import { deepClone, uid } from './util.js';

export const SIZE_CLASSES = {
  ground: { label: 'Ground cover', layer: 0, order: 0 },
  herb: { label: 'Herb', layer: 1, order: 1 },
  shrub: { label: 'Shrub', layer: 2, order: 2 },
  tree: { label: 'Tree', layer: 3, order: 3 },
  vine: { label: 'Vine', layer: 4, order: 4 },
};

export const SIZE_CLASS_IDS = Object.keys(SIZE_CLASSES);

// Per class ceilings, all settable from the World panel.
export function defaultClassLimits() {
  return {
    ground: { maxRadiusCells: 3, maxHeightPx: 10, minSpacing: 1, maxInstances: 160 },
    herb: { maxRadiusCells: 1, maxHeightPx: 26, minSpacing: 1, maxInstances: 120 },
    shrub: { maxRadiusCells: 2, maxHeightPx: 56, minSpacing: 2, maxInstances: 60 },
    tree: { maxRadiusCells: 5, maxHeightPx: 150, minSpacing: 4, maxInstances: 26 },
    vine: { maxRadiusCells: 3, maxHeightPx: 130, minSpacing: 3, maxInstances: 22 },
  };
}

export function makeSpecies(overrides = {}) {
  const base = {
    id: uid('sp'),
    name: 'New species',
    enabled: true,
    sizeClass: 'shrub',
    slots: {
      trunk: 'mat-trunk',
      branch: 'mat-branch',
      leaf: 'mat-leaf',
      leafEdge: 'mat-leafEdge',
      stem: 'mat-stem',
      ground: 'mat-ground',
    },
    spawn: { rate: 0.08, maxCount: 20, minSpacing: 3 },
    spread: { rate: 0.02, radiusMin: 2, radiusMax: 7 },
    growth: { rateMin: 0.6, rateMax: 1.4, stepMin: 2, stepMax: 4, maxAge: 900 },
    form: {
      baseWidth: 4,
      taper: 0.9,
      minWidth: 1,
      branchChance: 0.7,
      branchInterval: 8,
      branchAngleMin: 16,
      branchAngleMax: 40,
      maxDepth: 4,
      wander: 10,
      phototropism: 0.3,
      gravity: 0.06,
      leafDepth: 2,
      leafSizeMin: 2,
      leafSizeMax: 4,
      leafDensity: 0.45,
      leafEdges: true,
      petiole: 2,
      wrap: false,
      wrapPitch: 0.22,
      wrapAmp: 26,
      climbSearch: 3,
    },
    limits: { maxRadiusCells: 2, maxHeightPx: 56, maxTips: 20 },
    shade: {
      coreWood: 4,
      coreLeaf: 2.5,
      tones: 5,
      jitter: 0.05,
      behindShade: 0.18,
      adaptiveCore: false,
    },
  };
  return mergeDeep(base, overrides);
}

function mergeDeep(target, src) {
  const out = deepClone(target);
  for (const [k, v] of Object.entries(src || {})) {
    if (v && typeof v === 'object' && !Array.isArray(v)) out[k] = mergeDeep(out[k] || {}, v);
    else out[k] = v;
  }
  return out;
}

export function defaultSpeciesList() {
  return [
    makeSpecies({
      id: 'sp-moss',
      name: 'Moss mat',
      sizeClass: 'ground',
      spawn: { rate: 0.5, maxCount: 90, minSpacing: 1 },
      spread: { rate: 0.12, radiusMin: 1, radiusMax: 4 },
      growth: { rateMin: 0.5, rateMax: 1.1, stepMin: 1, stepMax: 2, maxAge: 3000 },
      limits: { maxRadiusCells: 3, maxHeightPx: 8, maxTips: 1 },
      shade: { coreWood: 3, coreLeaf: 3, tones: 4, jitter: 0.09, behindShade: 0.15 },
    }),
    makeSpecies({
      id: 'sp-grass',
      name: 'Grass tuft',
      sizeClass: 'herb',
      // Blades are drawn from the stem box, so a tuft reads green rather than
      // as a cluster of tiny brown twigs.
      slots: { trunk: 'mat-stem', branch: 'mat-stem', leaf: 'mat-leaf' },
      spawn: { rate: 0.35, maxCount: 70, minSpacing: 1 },
      spread: { rate: 0.06, radiusMin: 1, radiusMax: 5 },
      growth: { rateMin: 1.0, rateMax: 2.0, stepMin: 2, stepMax: 4, maxAge: 700 },
      form: {
        baseWidth: 2,
        taper: 0.86,
        branchChance: 0.85,
        branchInterval: 3,
        branchAngleMin: 8,
        branchAngleMax: 34,
        maxDepth: 1,
        wander: 6,
        phototropism: 0.5,
        gravity: 0.16,
        leafDepth: 9,
        leafDensity: 0.05,
        leafSizeMin: 1,
        leafSizeMax: 2,
        petiole: 0,
      },
      limits: { maxRadiusCells: 1, maxHeightPx: 24, maxTips: 10 },
      shade: { coreWood: 2, coreLeaf: 2, tones: 4, jitter: 0.06, behindShade: 0.15 },
    }),
    makeSpecies({
      id: 'sp-fern',
      name: 'Fern bush',
      sizeClass: 'shrub',
      spawn: { rate: 0.12, maxCount: 30, minSpacing: 2 },
      spread: { rate: 0.03, radiusMin: 2, radiusMax: 6 },
      growth: { rateMin: 0.8, rateMax: 1.6, stepMin: 2, stepMax: 4, maxAge: 1200 },
      form: {
        baseWidth: 3,
        taper: 0.9,
        branchChance: 0.8,
        branchInterval: 5,
        branchAngleMin: 22,
        branchAngleMax: 52,
        maxDepth: 3,
        wander: 12,
        phototropism: 0.28,
        gravity: 0.14,
        leafDepth: 2,
        leafDensity: 0.6,
        leafSizeMin: 2,
        leafSizeMax: 3,
        petiole: 1,
      },
      limits: { maxRadiusCells: 2, maxHeightPx: 46, maxTips: 22 },
    }),
    makeSpecies({
      id: 'sp-oak',
      name: 'Broadleaf tree',
      sizeClass: 'tree',
      spawn: { rate: 0.05, maxCount: 12, minSpacing: 5 },
      spread: { rate: 0.012, radiusMin: 4, radiusMax: 12 },
      growth: { rateMin: 0.7, rateMax: 1.3, stepMin: 3, stepMax: 6, maxAge: 4000 },
      form: {
        baseWidth: 7,
        taper: 0.93,
        minWidth: 1,
        branchChance: 0.75,
        branchInterval: 11,
        branchAngleMin: 18,
        branchAngleMax: 44,
        maxDepth: 5,
        wander: 9,
        phototropism: 0.22,
        gravity: 0.05,
        leafDepth: 3,
        leafDensity: 0.5,
        leafSizeMin: 3,
        leafSizeMax: 5,
        petiole: 2,
      },
      limits: { maxRadiusCells: 4, maxHeightPx: 130, maxTips: 40 },
      shade: { coreWood: 5, coreLeaf: 3, tones: 5, jitter: 0.05, behindShade: 0.2 },
    }),
    makeSpecies({
      id: 'sp-ivy',
      name: 'Climbing ivy',
      sizeClass: 'vine',
      spawn: { rate: 0.06, maxCount: 12, minSpacing: 3 },
      spread: { rate: 0.02, radiusMin: 2, radiusMax: 8 },
      growth: { rateMin: 1.1, rateMax: 2.2, stepMin: 2, stepMax: 3, maxAge: 2500 },
      form: {
        baseWidth: 2,
        taper: 0.98,
        branchChance: 0.35,
        branchInterval: 14,
        branchAngleMin: 30,
        branchAngleMax: 70,
        maxDepth: 3,
        wander: 14,
        phototropism: 0.12,
        gravity: 0.1,
        leafDepth: 0,
        leafDensity: 0.35,
        leafSizeMin: 2,
        leafSizeMax: 3,
        petiole: 1,
        wrap: true,
        wrapPitch: 0.24,
        wrapAmp: 30,
        climbSearch: 4,
      },
      limits: { maxRadiusCells: 3, maxHeightPx: 120, maxTips: 18 },
      shade: { coreWood: 2, coreLeaf: 2.5, tones: 5, jitter: 0.06, behindShade: 0.22 },
    }),
  ];
}

// Drives the generated species form. Ranges render as a linked min/max pair.
export const SPECIES_SCHEMA = [
  {
    group: 'Identity',
    fields: [
      { path: 'name', label: 'Name', type: 'text' },
      { path: 'sizeClass', label: 'Size class', type: 'select', options: SIZE_CLASS_IDS },
      { path: 'enabled', label: 'Enabled', type: 'bool' },
    ],
  },
  {
    group: 'Materials',
    fields: [
      { path: 'slots.trunk', label: 'Trunk', type: 'sampler' },
      { path: 'slots.branch', label: 'Branch', type: 'sampler' },
      { path: 'slots.leaf', label: 'Leaf', type: 'sampler' },
      { path: 'slots.leafEdge', label: 'Leaf edge', type: 'sampler' },
      { path: 'slots.stem', label: 'Stem to leaf', type: 'sampler' },
      { path: 'slots.ground', label: 'Ground', type: 'sampler' },
    ],
  },
  {
    group: 'Spawn and spread',
    fields: [
      { path: 'spawn.rate', label: 'Spawn rate', type: 'number', min: 0, max: 4, step: 0.01,
        hint: 'attempts per simulation second' },
      { path: 'spawn.maxCount', label: 'Max instances', type: 'number', min: 0, max: 400, step: 1 },
      { path: 'spawn.minSpacing', label: 'Min spacing (cells)', type: 'number', min: 0, max: 20, step: 1 },
      { path: 'spread.rate', label: 'Spread rate', type: 'number', min: 0, max: 2, step: 0.005,
        hint: 'offspring per parent per second' },
      { type: 'range', label: 'Spread distance (cells)', pathMin: 'spread.radiusMin',
        pathMax: 'spread.radiusMax', min: 0, max: 40, step: 1 },
    ],
  },
  {
    group: 'Growth',
    fields: [
      { type: 'range', label: 'Growth rate', pathMin: 'growth.rateMin', pathMax: 'growth.rateMax',
        min: 0.05, max: 6, step: 0.05, hint: 'segments per simulation second' },
      { type: 'range', label: 'Segment length (px)', pathMin: 'growth.stepMin',
        pathMax: 'growth.stepMax', min: 1, max: 14, step: 0.5 },
      { path: 'growth.maxAge', label: 'Max age', type: 'number', min: 10, max: 10000, step: 10 },
    ],
  },
  {
    group: 'Form and branching',
    fields: [
      { path: 'form.baseWidth', label: 'Base width (px)', type: 'number', min: 1, max: 24, step: 0.5 },
      { path: 'form.taper', label: 'Taper per segment', type: 'number', min: 0.5, max: 1, step: 0.005 },
      { path: 'form.minWidth', label: 'Min width (px)', type: 'number', min: 0.5, max: 6, step: 0.25 },
      { path: 'form.branchChance', label: 'Branch chance', type: 'number', min: 0, max: 1, step: 0.01 },
      { path: 'form.branchInterval', label: 'Branch interval (px)', type: 'number', min: 1, max: 40, step: 0.5 },
      { type: 'range', label: 'Branch angle (deg)', pathMin: 'form.branchAngleMin',
        pathMax: 'form.branchAngleMax', min: 0, max: 120, step: 1 },
      { path: 'form.maxDepth', label: 'Max branch depth', type: 'number', min: 0, max: 9, step: 1 },
      { path: 'form.wander', label: 'Wander (deg)', type: 'number', min: 0, max: 60, step: 0.5 },
      { path: 'form.phototropism', label: 'Phototropism', type: 'number', min: 0, max: 1, step: 0.01,
        hint: 'pull of tips back toward vertical' },
      { path: 'form.gravity', label: 'Droop', type: 'number', min: 0, max: 1, step: 0.01 },
    ],
  },
  {
    group: 'Leaves',
    fields: [
      { path: 'form.leafDepth', label: 'First leaf depth', type: 'number', min: 0, max: 9, step: 1 },
      { path: 'form.leafDensity', label: 'Leaf density', type: 'number', min: 0, max: 1, step: 0.01 },
      { type: 'range', label: 'Leaf size (px)', pathMin: 'form.leafSizeMin',
        pathMax: 'form.leafSizeMax', min: 1, max: 12, step: 0.5 },
      { path: 'form.petiole', label: 'Stem to leaf (px)', type: 'number', min: 0, max: 10, step: 0.5 },
      { path: 'form.leafEdges', label: 'Draw leaf edges', type: 'bool' },
    ],
  },
  {
    group: 'Climbing and wrapping',
    fields: [
      { path: 'form.wrap', label: 'Wrap around supports', type: 'bool' },
      { path: 'form.climbSearch', label: 'Support search (cells)', type: 'number', min: 0, max: 12, step: 1 },
      { path: 'form.wrapPitch', label: 'Wrap pitch', type: 'number', min: 0.02, max: 1.2, step: 0.01 },
      { path: 'form.wrapAmp', label: 'Wrap sway (deg)', type: 'number', min: 0, max: 90, step: 1 },
    ],
  },
  {
    group: 'Limits',
    fields: [
      { path: 'limits.maxRadiusCells', label: 'Footprint radius (cells)', type: 'number', min: 0, max: 20, step: 1,
        hint: 'clamped by the size class ceiling' },
      { path: 'limits.maxHeightPx', label: 'Max height (px)', type: 'number', min: 4, max: 400, step: 2 },
      { path: 'limits.maxTips', label: 'Max active tips', type: 'number', min: 1, max: 120, step: 1 },
    ],
  },
  {
    group: 'Shading',
    fields: [
      { path: 'shade.tones', label: 'Tone steps', type: 'number', min: 2, max: 16, step: 1 },
      { path: 'shade.coreWood', label: 'Wood core depth (px)', type: 'number', min: 0.5, max: 16, step: 0.5 },
      { path: 'shade.coreLeaf', label: 'Leaf core depth (px)', type: 'number', min: 0.5, max: 16, step: 0.5 },
      { path: 'shade.adaptiveCore', label: 'Adaptive core depth', type: 'bool',
        hint: 'off keeps thin parts light, on lets every shape use the full ramp' },
      { path: 'shade.jitter', label: 'Tone jitter', type: 'number', min: 0, max: 0.4, step: 0.005 },
      { path: 'shade.behindShade', label: 'Behind-support darkening', type: 'number', min: 0, max: 0.6, step: 0.01 },
    ],
  },
];

export function getPath(obj, path) {
  return path.split('.').reduce((o, k) => (o == null ? undefined : o[k]), obj);
}

export function setPath(obj, path, value) {
  const keys = path.split('.');
  let cur = obj;
  for (let i = 0; i < keys.length - 1; i++) {
    if (cur[keys[i]] == null) cur[keys[i]] = {};
    cur = cur[keys[i]];
  }
  cur[keys[keys.length - 1]] = value;
}

// Species limits never exceed their size class ceiling.
export function effectiveLimits(species, classLimits) {
  const cl = classLimits[species.sizeClass] || classLimits.shrub;
  return {
    maxRadiusCells: Math.min(species.limits.maxRadiusCells, cl.maxRadiusCells),
    maxHeightPx: Math.min(species.limits.maxHeightPx, cl.maxHeightPx),
    maxTips: species.limits.maxTips,
    minSpacing: Math.max(species.spawn.minSpacing, cl.minSpacing),
    maxInstances: Math.min(species.spawn.maxCount, cl.maxInstances),
  };
}
