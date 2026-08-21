// Economy: prices, wages, the treasury and the caravans.
//
// Prices are not set anywhere, they fall out of stock against demand: every
// resource has a target stock that grows with the population, and its price is
// the base price scaled by how far the store is from that target. Wages are
// paid out of the treasury as work is done, people buy their food back from
// the market, and caravans move coin in and out by trading the surplus.

import { RES, RES_IDS, addStock, takeStock } from './resources.js';
import { caravanName } from './names.js';
import { clamp } from '../util.js';

export function defaultEconomyConfig() {
  return {
    startCoin: 80,
    wage: 0.5,
    elasticity: 0.85,
    priceSmoothing: 0.25,
    // Target stock per person, multiplied by a per resource weight below.
    stockPerPerson: 4,
    rawWeight: 1.6,
    madeWeight: 0.6,
    // Stock above this multiple of the target is not worth carrying home.
    hoardLimit: 2.5,
    tradeInterval: 100,
    tradeVolume: 40,
    tradeMargin: 0.25,
    caravanCoin: 240,
    paysWages: true,
    historyLength: 320,
  };
}

export function makeEconomy(cfg) {
  const prices = {};
  for (const id of RES_IDS) prices[id] = RES[id].basePrice;
  return {
    coin: cfg.startCoin,
    prices,
    produced: zero(),
    consumed: zero(),
    rateIn: zero(),
    rateOut: zero(),
    history: [],
    events: [],
    unpaidWages: 0,
    tradeTimer: 0,
    trades: 0,
    tradeBalance: 0,
  };
}

function zero() {
  const o = {};
  for (const id of RES_IDS) o[id] = 0;
  return o;
}

export function stockTargets(cfg, population) {
  const out = {};
  const pop = Math.max(1, population);
  for (const id of RES_IDS) {
    const weight = RES[id].kind === 'raw' ? cfg.rawWeight : cfg.madeWeight;
    out[id] = Math.max(4, pop * cfg.stockPerPerson * weight);
  }
  return out;
}

export function updatePrices(econ, cfg, stock, population, dt) {
  const targets = stockTargets(cfg, population);
  for (const id of RES_IDS) {
    const target = targets[id];
    const have = stock[id] || 0;
    const scarcity = clamp((target + 1) / (have + 1), 0.2, 6);
    const want = RES[id].basePrice * Math.pow(scarcity, cfg.elasticity);
    const k = clamp(cfg.priceSmoothing * dt, 0, 1);
    econ.prices[id] = econ.prices[id] + (want - econ.prices[id]) * k;
  }
}

export function priceOf(econ, id) {
  return econ.prices[id] || RES[id].basePrice;
}

export function recordProduced(econ, id, n) {
  econ.produced[id] = (econ.produced[id] || 0) + n;
}

export function recordConsumed(econ, id, n) {
  econ.consumed[id] = (econ.consumed[id] || 0) + n;
}

// Per day flow rates, smoothed, so the panel shows a trend rather than the
// spike of whatever happened in the last tick.
export function rollFlows(econ) {
  for (const id of RES_IDS) {
    econ.rateIn[id] = econ.rateIn[id] * 0.5 + econ.produced[id] * 0.5;
    econ.rateOut[id] = econ.rateOut[id] * 0.5 + econ.consumed[id] * 0.5;
    econ.produced[id] = 0;
    econ.consumed[id] = 0;
  }
}

export function pushHistory(econ, cfg, sample) {
  econ.history.push(sample);
  const max = Math.max(20, cfg.historyLength | 0);
  if (econ.history.length > max) econ.history.splice(0, econ.history.length - max);
}

export function logEvent(econ, text, day) {
  econ.events.push({ text, day });
  if (econ.events.length > 60) econ.events.shift();
}

// Wages are paid as work happens. An empty treasury does not stop the work, it
// just leaves the wage unpaid, which shows up as unhappiness.
export function payWage(econ, cfg, person, workUnits) {
  if (!cfg.paysWages) return 0;
  const due = cfg.wage * workUnits;
  if (econ.coin >= due) {
    econ.coin -= due;
    person.coin += due;
    person.wage += due;
    return due;
  }
  econ.unpaidWages += due;
  return 0;
}

// A meal is bought from the settlement store at the market price when there is
// a market; without one people simply take what they need.
export function buyFood(econ, cfg, person, stock, amount, hasMarket) {
  const got = takeStock(stock, 'food', amount);
  if (got <= 0) return 0;
  if (hasMarket) {
    const cost = priceOf(econ, 'food') * got;
    const paid = Math.min(person.coin, cost);
    person.coin -= paid;
    econ.coin += paid;
  }
  recordConsumed(econ, 'food', got);
  return got;
}

// One caravan visit: it sells what the settlement is short of and buys the
// surplus, both at the market price shifted by the trade margin.
export function runCaravan(econ, cfg, stock, population, mods, rng, day) {
  const targets = stockTargets(cfg, population);
  const margin = cfg.tradeMargin / Math.max(0.2, mods.trade || 1);
  const name = caravanName(rng);
  let purse = cfg.caravanCoin;
  let volume = cfg.tradeVolume;
  const sold = [];
  const bought = [];

  // Settlement sells its surplus first, which is where its coin comes from.
  const surplus = RES_IDS
    .map((id) => ({ id, over: (stock[id] || 0) - targets[id] * 1.3 }))
    .filter((e) => e.over > 1)
    .sort((a, b) => b.over * priceOf(econ, b.id) - a.over * priceOf(econ, a.id));
  for (const entry of surplus) {
    if (volume <= 0 || purse <= 0) break;
    const unit = priceOf(econ, entry.id) * (1 - margin);
    const affordable = Math.floor(purse / Math.max(0.01, unit));
    const n = Math.min(Math.floor(entry.over), volume, affordable);
    if (n <= 0) continue;
    takeStock(stock, entry.id, n);
    const gain = n * unit;
    econ.coin += gain;
    purse -= gain;
    volume -= n;
    econ.tradeBalance += gain;
    sold.push(`${n} ${RES[entry.id].label.toLowerCase()}`);
  }

  // Then it sells the settlement what it is short of, if there is coin for it.
  const shortage = RES_IDS
    .map((id) => ({ id, short: targets[id] * 0.5 - (stock[id] || 0) }))
    .filter((e) => e.short > 1)
    .sort((a, b) => b.short - a.short);
  for (const entry of shortage) {
    if (volume <= 0 || econ.coin <= 0) break;
    const unit = priceOf(econ, entry.id) * (1 + margin);
    const affordable = Math.floor(econ.coin / Math.max(0.01, unit));
    const n = Math.min(Math.ceil(entry.short), volume, affordable, 30);
    if (n <= 0) continue;
    addStock(stock, entry.id, n);
    const spend = n * unit;
    econ.coin -= spend;
    volume -= n;
    econ.tradeBalance -= spend;
    bought.push(`${n} ${RES[entry.id].label.toLowerCase()}`);
  }

  econ.trades++;
  const parts = [];
  if (sold.length) parts.push(`bought ${sold.join(', ')}`);
  if (bought.length) parts.push(`sold us ${bought.join(', ')}`);
  logEvent(econ, `${name} ${parts.length ? parts.join(' and ') : 'found nothing to trade'}`, day);
  return { sold, bought };
}

export function netWorth(econ, stock) {
  let value = econ.coin;
  for (const id of RES_IDS) value += (stock[id] || 0) * priceOf(econ, id);
  return value;
}
