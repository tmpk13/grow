// Resources: what the settlement gathers, refines, stores and trades.
//
// Everything downstream (recipes, building costs, prices, the stock panel) is
// generated from this table, so adding a resource here is enough to make it
// appear everywhere it is relevant.

export const RESOURCES = [
  { id: 'wood', label: 'Wood', kind: 'raw', color: '#8a6644', basePrice: 2, bulk: 1, decay: 0 },
  { id: 'stone', label: 'Stone', kind: 'raw', color: '#8d97a3', basePrice: 3, bulk: 1.4, decay: 0 },
  { id: 'clay', label: 'Clay', kind: 'raw', color: '#b07a5a', basePrice: 2, bulk: 1.2, decay: 0 },
  { id: 'ore', label: 'Ore', kind: 'raw', color: '#7d6f8f', basePrice: 6, bulk: 1.5, decay: 0 },
  { id: 'fiber', label: 'Fiber', kind: 'raw', color: '#c8b46a', basePrice: 2, bulk: 0.6, decay: 0 },
  { id: 'food', label: 'Food', kind: 'raw', color: '#9fd06a', basePrice: 3, bulk: 0.8, decay: 0.0004 },
  { id: 'plank', label: 'Plank', kind: 'made', color: '#c39a63', basePrice: 6, bulk: 1, decay: 0 },
  { id: 'brick', label: 'Brick', kind: 'made', color: '#c06a4e', basePrice: 7, bulk: 1.6, decay: 0 },
  { id: 'charcoal', label: 'Charcoal', kind: 'made', color: '#57575f', basePrice: 5, bulk: 0.7, decay: 0 },
  { id: 'metal', label: 'Metal', kind: 'made', color: '#9fb6c9', basePrice: 14, bulk: 1.3, decay: 0 },
  { id: 'tool', label: 'Tool', kind: 'made', color: '#d8d2b8', basePrice: 22, bulk: 0.9, decay: 0 },
  { id: 'cloth', label: 'Cloth', kind: 'made', color: '#cf8fb0', basePrice: 10, bulk: 0.5, decay: 0 },
];

export const RES_IDS = RESOURCES.map((r) => r.id);
export const RES = Object.fromEntries(RESOURCES.map((r) => [r.id, r]));

export function makeStock(fill = 0) {
  const stock = {};
  for (const id of RES_IDS) stock[id] = fill;
  return stock;
}

// Stocks are plain objects so they serialize as they are; these helpers keep
// them non negative and tolerate ids that a loaded project does not know.
export function addStock(stock, id, n) {
  if (n === 0) return 0;
  const have = stock[id] || 0;
  stock[id] = have + n;
  return n;
}

export function takeStock(stock, id, n) {
  const have = stock[id] || 0;
  const got = Math.min(have, n);
  stock[id] = have - got;
  return got;
}

export function stockBulk(stock) {
  let total = 0;
  for (const id of RES_IDS) total += (stock[id] || 0) * (RES[id].bulk || 1);
  return total;
}

export function stockTotal(stock) {
  let total = 0;
  for (const id of RES_IDS) total += stock[id] || 0;
  return total;
}

// A cost object is short (two or three entries), so it is kept as a plain
// object and read through these instead of being normalized into arrays.
export function canAfford(stock, cost) {
  for (const [id, n] of Object.entries(cost)) {
    if ((stock[id] || 0) < n) return false;
  }
  return true;
}

export function missingFrom(stock, cost) {
  const missing = {};
  for (const [id, n] of Object.entries(cost)) {
    const short = n - (stock[id] || 0);
    if (short > 0) missing[id] = short;
  }
  return missing;
}

export function formatCost(cost) {
  return Object.entries(cost)
    .map(([id, n]) => `${n} ${RES[id] ? RES[id].label.toLowerCase() : id}`)
    .join(', ');
}
