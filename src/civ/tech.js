// Technology: a small directed graph of unlocks.
//
// A tech costs research points, needs its prerequisites, and pays out in two
// ways: it unlocks building types and it raises named modifiers that the rest
// of the sim multiplies its rates by. Nothing else in the sim knows the name
// of a tech, only the modifiers and the unlock list, so the tree can be
// reshaped here without touching the simulation.

export const MOD_KEYS = {
  gather: 'Gathering speed',
  build: 'Construction speed',
  craft: 'Crafting speed',
  carry: 'Carry capacity',
  farm: 'Farm yield',
  research: 'Research output',
  trade: 'Trade margin',
  comfort: 'Housing comfort',
  yield: 'Harvest yield',
};

export const TECHS = [
  {
    id: 'stonework',
    label: 'Stone working',
    cost: 24,
    requires: [],
    unlocks: ['quarry'],
    effects: { gather: 0.1 },
    note: 'Shaped stone: opens the quarry and speeds up gathering.',
  },
  {
    id: 'firecraft',
    label: 'Firecraft',
    cost: 40,
    requires: ['stonework'],
    unlocks: ['charcoalHearth'],
    effects: {},
    note: 'Controlled burning: charcoal for every later furnace.',
  },
  {
    id: 'carpentry',
    label: 'Carpentry',
    cost: 55,
    requires: ['stonework'],
    unlocks: ['sawpit', 'house'],
    effects: { build: 0.15 },
    note: 'Sawn planks and framed houses.',
  },
  {
    id: 'agriculture',
    label: 'Agriculture',
    cost: 70,
    requires: ['stonework'],
    unlocks: ['farm', 'granary'],
    effects: { farm: 0.1 },
    note: 'Sown fields instead of foraging.',
  },
  {
    id: 'pottery',
    label: 'Pottery',
    cost: 80,
    requires: ['firecraft'],
    unlocks: ['claypit', 'kiln'],
    effects: {},
    note: 'Fired clay: bricks, and the vessels that keep food.',
  },
  {
    id: 'weaving',
    label: 'Weaving',
    cost: 90,
    requires: ['agriculture'],
    unlocks: ['weaver'],
    effects: { comfort: 0.1 },
    note: 'Cloth from fiber.',
  },
  {
    id: 'cartage',
    label: 'Cartage',
    cost: 110,
    requires: ['carpentry'],
    unlocks: [],
    effects: { carry: 0.6 },
    note: 'Barrows and carts: every worker hauls far more per trip.',
  },
  {
    id: 'writing',
    label: 'Writing',
    cost: 130,
    requires: ['pottery'],
    unlocks: ['school'],
    effects: { research: 0.2 },
    note: 'Records that outlive the person who made them.',
  },
  {
    id: 'masonry',
    label: 'Masonry',
    cost: 160,
    requires: ['carpentry', 'pottery'],
    unlocks: ['well', 'manor'],
    effects: { comfort: 0.15, build: 0.1 },
    note: 'Mortared walls, wells and larger houses.',
  },
  {
    id: 'mining',
    label: 'Mining',
    cost: 190,
    requires: ['stonework', 'carpentry'],
    unlocks: ['mine'],
    effects: { yield: 0.1 },
    note: 'Shafts and props reach the ore.',
  },
  {
    id: 'trade',
    label: 'Trade',
    cost: 210,
    requires: ['writing'],
    unlocks: ['market'],
    effects: { trade: 0.2 },
    note: 'A market, prices, and caravans that answer them.',
  },
  {
    id: 'smelting',
    label: 'Smelting',
    cost: 260,
    requires: ['mining', 'firecraft'],
    unlocks: ['smelter'],
    effects: {},
    note: 'Ore and charcoal into metal.',
  },
  {
    id: 'mathematics',
    label: 'Mathematics',
    cost: 300,
    requires: ['writing'],
    unlocks: [],
    effects: { research: 0.25, build: 0.1 },
    note: 'Measure, plan, and predict.',
  },
  {
    id: 'metallurgy',
    label: 'Metallurgy',
    cost: 380,
    requires: ['smelting'],
    unlocks: ['smithy'],
    effects: { gather: 0.2, craft: 0.15 },
    note: 'Metal tools in every hand.',
  },
  {
    id: 'irrigation',
    label: 'Irrigation',
    cost: 420,
    requires: ['agriculture', 'masonry'],
    unlocks: [],
    effects: { farm: 0.45 },
    note: 'Water carried to the fields.',
  },
  {
    id: 'engineering',
    label: 'Engineering',
    cost: 560,
    requires: ['mathematics', 'metallurgy'],
    unlocks: ['workshop'],
    effects: { build: 0.3, craft: 0.25 },
    note: 'Machines that multiply a day of work.',
  },
  {
    id: 'printing',
    label: 'Printing',
    cost: 700,
    requires: ['engineering'],
    unlocks: [],
    effects: { research: 0.6 },
    note: 'Knowledge copied faster than it is forgotten.',
  },
];

export const TECH_BY_ID = Object.fromEntries(TECHS.map((t) => [t.id, t]));

export function defaultTechConfig() {
  return {
    costScale: 1,
    researchPerScholar: 0.6,
    insightPerPerson: 0.006,
    autoResearch: true,
    // Auto research prefers the cheapest reachable tech; raising this makes it
    // prefer techs that unlock buildings the settlement is short of.
    needBias: 0.5,
  };
}

export function makeTechState() {
  return { known: [], points: 0, spent: 0, target: null, log: [] };
}

export function isKnown(tech, id) {
  return tech.known.indexOf(id) !== -1;
}

export function techCost(def, cfg) {
  return Math.max(1, Math.round(def.cost * (cfg.costScale || 1)));
}

export function available(tech) {
  return TECHS.filter(
    (t) => !isKnown(tech, t.id) && t.requires.every((r) => isKnown(tech, r)),
  );
}

export function locked(tech) {
  return TECHS.filter((t) => !isKnown(tech, t.id) && !t.requires.every((r) => isKnown(tech, r)));
}

// Multipliers applied all over the sim. Effects are additive fractions, so
// three techs worth +0.1 gathering give x1.3 rather than x1.331.
export function modifiers(tech) {
  const mods = {};
  for (const key of Object.keys(MOD_KEYS)) mods[key] = 1;
  for (const id of tech.known) {
    const def = TECH_BY_ID[id];
    if (!def) continue;
    for (const [k, v] of Object.entries(def.effects || {})) {
      mods[k] = (mods[k] || 1) + v;
    }
  }
  return mods;
}

export function unlockedBuildings(tech) {
  const set = new Set();
  for (const id of tech.known) {
    const def = TECH_BY_ID[id];
    if (!def) continue;
    for (const b of def.unlocks) set.add(b);
  }
  return set;
}

export function progressFor(tech, cfg, id) {
  const def = TECH_BY_ID[id];
  if (!def) return 0;
  return tech.points / techCost(def, cfg);
}
