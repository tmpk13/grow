// Drawing the settlement.
//
// Everything is generated: the ground is dithered out of the sampling box
// ramps, buildings are assembled from their own dimensions and material
// slots, and people are three pixels wide with a palette hashed from their id.
// No sprite is stored anywhere in the project, so changing a cell size or
// repainting a material box changes the whole town.
//
// Sprites are cached by the values they are built from, which is why the cache
// key carries the materials version and the cell size.

import { findSampler, rampPick, samplerRamp } from '../sampler.js';
import { RES } from './resources.js';
import { CELL } from './terrain.js';
import { clamp, clamp01, hash2, hexToPacked, lerp, mixPacked, packRGBA, unpackRGBA } from '../util.js';

const spriteCache = new Map();

export function invalidateCivSprites() {
  spriteCache.clear();
}

function rampOf(state, samplerId) {
  const sampler = findSampler(state.materials, samplerId);
  const ramp = sampler ? samplerRamp(state.materials, sampler) : [];
  return ramp.length ? ramp : [packRGBA(90, 90, 90, 255), packRGBA(150, 150, 150, 255)];
}

function shade(ramp, t) {
  return rampPick(ramp, clamp01(t));
}

// ---- background ----------------------------------------------------------

// Sky, ground and water for the whole map. Cached because it only changes when
// the terrain, the palette or the map size changes; the time of day is a tint
// drawn over the finished frame instead.
export function paintTerrain(sim, buf) {
  const world = sim.world;
  const state = sim.state;
  const cfg = sim.cfg;
  const soil = rampOf(state, cfg.world.soilSampler);
  const grass = rampOf(state, 'mat-ground');
  const rock = rampOf(state, 'mat-stone');
  const sand = rampOf(state, 'mat-soil');
  const skyTop = hexToPacked(cfg.world.skyTop);
  const skyBottom = hexToPacked(cfg.world.skyBottom);
  const waterTop = hexToPacked(cfg.view.waterTop);
  const waterDeep = hexToPacked(cfg.view.waterDeep);

  for (let y = 0; y < world.skyPx; y++) {
    const t = world.skyPx > 1 ? y / (world.skyPx - 1) : 0;
    buf.fill(mixPacked(skyTop, skyBottom, t), y * world.pxW, (y + 1) * world.pxW);
  }

  const fade = cfg.world.depthFade || 0;
  for (let y = world.skyPx; y < world.pxH; y++) {
    const row = clamp(Math.floor((y - world.skyPx) / world.depthPx), 0, world.rows - 1);
    const far = world.rows > 1 ? 1 - row / (world.rows - 1) : 0;
    for (let x = 0; x < world.pxW; x++) {
      const col = clamp(Math.floor(x / world.cellPx), 0, world.cols - 1);
      const i = sim.terrain.idx(col, row);
      const type = sim.terrain.type[i];
      const noise = (hash2(x, y, 7331) - 0.5) * 0.24;
      let c;
      if (type === CELL.water) {
        const depth = clamp01((sim.cfg.terrain.waterLevel - sim.terrain.elev[i]) * 6);
        c = mixPacked(waterTop, waterDeep, clamp01(depth + noise * 0.5));
      } else {
        // Fertile ground reads green, bare ground reads as soil, and the two
        // are dithered into each other rather than tiled.
        const fert = sim.terrain.fert[i];
        const ramp = type === CELL.rock ? rock : type === CELL.sand ? sand : fert > 0.35 ? grass : soil;
        const t = clamp01(0.4 + far * fade * 2 + noise + (fert - 0.4) * 0.25);
        c = shade(ramp, t);
      }
      buf[y * world.pxW + x] = c;
    }
  }

  paintDeposits(sim, buf);
}

function paintDeposits(sim, buf) {
  if (!sim.cfg.view.deposits) return;
  const world = sim.world;
  const state = sim.state;
  const ramps = {
    stone: rampOf(state, 'mat-stone'),
    clay: rampOf(state, 'mat-soil'),
    ore: rampOf(state, 'mat-metal'),
  };
  for (const dep of sim.terrain.deposits) {
    if (dep.amount <= 0) continue;
    const ramp = ramps[dep.kind] || ramps.stone;
    const cx = world.anchorX(dep.col);
    const cy = world.anchorY(dep.row);
    const left = Math.max(0, dep.amount / Math.max(1, dep.max));
    const rx = Math.max(1, Math.round((world.cellPx * 0.42) * (0.5 + left * 0.5)));
    const ry = Math.max(1, Math.round(rx * world.depthRatio + 1));
    for (let y = -ry; y <= ry; y++) {
      for (let x = -rx; x <= rx; x++) {
        const px = cx + x;
        const py = cy + y;
        if (px < 0 || px >= world.pxW || py < 0 || py >= world.pxH) continue;
        const n = hash2(px, py, dep.seed);
        if ((x * x) / (rx * rx) + (y * y) / (ry * ry) > 0.6 + n * 0.5) continue;
        const t = 0.35 + n * 0.5 - (y / ry) * 0.15;
        buf[py * world.pxW + px] = shade(ramp, t);
      }
    }
  }
}

// Cells that get walked over often wear into a path. Drawn per frame over the
// cached ground rather than baked into it, because it keeps changing.
function paintPaths(sim, buf) {
  if (!sim.cfg.view.paths) return;
  const world = sim.world;
  const color = hexToPacked(sim.cfg.view.pathColor);
  for (let row = 0; row < world.rows; row++) {
    for (let col = 0; col < world.cols; col++) {
      const wear = sim.traffic[row * world.cols + col];
      if (wear < 1.2) continue;
      if (sim.terrain.type[sim.terrain.idx(col, row)] === CELL.water) continue;
      const strength = clamp01((wear - 1.2) / 8) * 0.55;
      const x0 = col * world.cellPx;
      const y0 = world.skyPx + row * world.depthPx;
      for (let y = y0; y < y0 + world.depthPx; y++) {
        if (y < 0 || y >= world.pxH) continue;
        for (let x = x0; x < x0 + world.cellPx; x++) {
          const n = hash2(x, y, 913);
          if (n > strength * 1.6) continue;
          const i = y * world.pxW + x;
          buf[i] = mixPacked(buf[i], color, strength);
        }
      }
    }
  }
}

// ---- buildings -----------------------------------------------------------

function buildingKey(sim, b) {
  const lit = b.built && sim.nightLights ? 1 : 0;
  const stage = b.built ? 9 : Math.min(8, Math.floor((b.workDone / Math.max(1, b.work)) * 8));
  return `b:${b.type}:${b.seed & 255}:${stage}:${lit}:${sim.world.cellPx}:${sim.world.depthPx}:${sim.state.materials.version}`;
}

// A building is drawn as a front wall standing on the near edge of its
// footprint with a roof laid over the depth of it, which is the same 2.5D
// projection the plants stand in.
export function buildingSprite(sim, b) {
  const key = buildingKey(sim, b);
  const hit = spriteCache.get(key);
  if (hit) return hit;

  const world = sim.world;
  const def = b.def;
  const eave = Math.max(1, Math.round(world.cellPx * 0.26));
  const bodyW = def.w * world.cellPx;
  const depth = def.h * world.depthPx;
  const wallH = Math.max(2, Math.round(def.wallH * world.cellPx));
  const roofH = Math.max(2, Math.round(def.roofH * world.cellPx));
  const w = bodyW + eave * 2;
  const h = depth + wallH + roofH;
  const px = new Uint32Array(w * h);

  const wall = rampOf(sim.state, def.palette.wall);
  const roof = rampOf(sim.state, def.palette.roof);
  const trim = rampOf(sim.state, def.palette.trim);
  const seed = b.seed;
  const progress = b.built ? 1 : clamp01(b.workDone / Math.max(1, b.work));
  const roofBottom = roofH + depth;
  const wallTop = roofBottom;

  const put = (x, y, c) => {
    if (x < 0 || x >= w || y < 0 || y >= h) return;
    px[y * w + x] = c;
  };

  if (b.built) {
    // Roof: a hipped plane, lighter along the ridge, drawn over the depth of
    // the footprint so a deeper building shows more roof.
    for (let y = 0; y < roofBottom; y++) {
      const t = y / Math.max(1, roofBottom - 1);
      const inset = Math.round((1 - t) * bodyW * 0.22);
      const tone = 0.78 - t * 0.42 + (hash2(0, y, seed) - 0.5) * 0.08;
      for (let x = inset; x < w - inset; x++) {
        put(x, y, shade(roof, tone + (hash2(x, y, seed) - 0.5) * 0.1));
      }
    }
    // Ridge and eave lines give the roof an edge without an outline pass.
    for (let x = Math.round(bodyW * 0.22); x < w - Math.round(bodyW * 0.22); x++) put(x, 0, shade(roof, 0.95));
    for (let x = 0; x < w; x++) put(x, roofBottom - 1, shade(trim, 0.2));

    for (let y = wallTop; y < h; y++) {
      const t = (y - wallTop) / Math.max(1, wallH - 1);
      for (let x = eave; x < w - eave; x++) {
        const tone = 0.68 - t * 0.34 + (hash2(x, y, seed + 7) - 0.5) * 0.09;
        put(x, y, shade(wall, tone));
      }
    }
    paintOpenings(sim, { put, w, h, eave, bodyW, wallTop, wallH, seed, trim, def, lit: sim.nightLights });
  } else {
    // Under construction: corner posts first, then the wall rising with the
    // work done on it.
    const raised = Math.round(wallH * progress);
    for (let y = h - raised; y < h; y++) {
      const t = (y - (h - raised)) / Math.max(1, raised);
      for (let x = eave; x < w - eave; x++) {
        put(x, y, shade(wall, 0.6 - t * 0.3 + (hash2(x, y, seed + 7) - 0.5) * 0.08));
      }
    }
    const postTop = h - wallH - Math.round(roofH * 0.4 * progress);
    for (const x of [eave, w - eave - 1, eave + Math.floor(bodyW / 2)]) {
      for (let y = postTop; y < h; y++) put(x, y, shade(trim, 0.45 + (y % 3) * 0.05));
    }
    for (let x = eave; x < w - eave; x++) put(x, postTop, shade(trim, 0.55));
  }

  const sprite = { w, h, px, ox: eave, oy: h };
  spriteCache.set(key, sprite);
  return sprite;
}

// Door and windows, spaced along the wall rather than placed by hand, and lit
// from inside once it is dark.
function paintOpenings(sim, ctx) {
  const { put, w, eave, bodyW, wallTop, wallH, seed, trim, def, lit } = ctx;
  const dark = shade(trim, 0.08);
  const glow = packRGBA(250, 214, 130, 255);
  const doorW = Math.max(1, Math.round(bodyW * 0.16));
  const doorH = Math.max(2, Math.round(wallH * 0.6));
  const doorX = eave + Math.round(bodyW * (0.3 + (seed % 3) * 0.15));
  for (let y = wallTop + wallH - doorH; y < wallTop + wallH; y++) {
    for (let x = doorX; x < doorX + doorW; x++) put(x, y, dark);
  }
  if (wallH < 5 || !def.housing && !def.slots) return;
  const winH = Math.max(1, Math.round(wallH * 0.22));
  const winW = Math.max(1, Math.round(bodyW * 0.12));
  const winY = wallTop + Math.max(1, Math.round(wallH * 0.25));
  const step = Math.max(winW * 2, Math.round(bodyW / 3));
  for (let x = eave + step - winW; x < w - eave - winW; x += step) {
    if (x + winW > doorX - 1 && x < doorX + doorW + 1) continue;
    for (let y = winY; y < winY + winH; y++) {
      for (let dx = 0; dx < winW; dx++) put(x + dx, y, lit ? glow : dark);
    }
  }
}

// ---- people --------------------------------------------------------------

// Three pixels wide and a head: enough to read a walk cycle, a facing and
// whether somebody is carrying something. Colors are hashed from the person id
// so a person looks the same for their whole life.
export function personSprite(sim, p, frame) {
  const world = sim.world;
  const bodyH = Math.max(4, Math.round(world.cellPx * 0.85));
  const bodyW = Math.max(2, Math.round(world.cellPx * 0.3));
  const key = `p:${p.seed & 1023}:${frame}:${p.facing}:${bodyW}:${bodyH}:${p.adult ? 1 : 0}`;
  const hit = spriteCache.get(key);
  if (hit) return hit;

  const scale = p.adult ? 1 : 0.7;
  const hh = Math.max(3, Math.round(bodyH * scale));
  const ww = Math.max(2, Math.round(bodyW * scale) + 1);
  const w = ww + 2;
  const h = hh + 1;
  const px = new Uint32Array(w * h);
  const skinTone = hash2(p.seed, 3, 11);
  const skin = packRGBA(
    Math.round(lerp(232, 128, skinTone)),
    Math.round(lerp(190, 88, skinTone)),
    Math.round(lerp(160, 62, skinTone)),
    255,
  );
  const hue = hash2(p.seed, 7, 23) * 360;
  const shirt = hsl(hue, 0.35, 0.42);
  const legs = hsl((hue + 40) % 360, 0.22, 0.26);
  const hair = hsl((hue + 200) % 360, 0.3, 0.18);

  const headH = Math.max(1, Math.round(hh * 0.32));
  const legH = Math.max(1, Math.round(hh * 0.3));
  const x0 = 1;
  for (let y = 0; y < h; y++) {
    for (let x = x0; x < x0 + ww; x++) {
      let c = 0;
      if (y < headH) c = y === 0 ? hair : skin;
      else if (y < hh - legH) c = shirt;
      else if (y < hh) {
        // Legs split and swap with the frame, which reads as a step.
        const left = x < x0 + ww / 2;
        const lift = frame === 1 ? left : !left;
        c = lift && y === hh - 1 ? 0 : legs;
      }
      if (c) px[y * w + x] = c;
    }
  }
  const sprite = { w, h, px, ox: Math.floor(w / 2), oy: h };
  spriteCache.set(key, sprite);
  return sprite;
}

function hsl(hue, sat, light) {
  const h = ((hue % 360) + 360) % 360 / 60;
  const c = (1 - Math.abs(2 * light - 1)) * sat;
  const x = c * (1 - Math.abs((h % 2) - 1));
  const m = light - c / 2;
  let r = 0;
  let g = 0;
  let b = 0;
  if (h < 1) [r, g, b] = [c, x, 0];
  else if (h < 2) [r, g, b] = [x, c, 0];
  else if (h < 3) [r, g, b] = [0, c, x];
  else if (h < 4) [r, g, b] = [0, x, c];
  else if (h < 5) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  return packRGBA(
    Math.round((r + m) * 255),
    Math.round((g + m) * 255),
    Math.round((b + m) * 255),
    255,
  );
}

// ---- compositing ---------------------------------------------------------

function blit(buf, world, sprite, sx, sy) {
  const x0 = sx - sprite.ox;
  const y0 = sy - sprite.oy;
  for (let y = 0; y < sprite.h; y++) {
    const wy = y0 + y;
    if (wy < 0 || wy >= world.pxH) continue;
    const srow = y * sprite.w;
    const drow = wy * world.pxW;
    for (let x = 0; x < sprite.w; x++) {
      const v = sprite.px[srow + x];
      if (v === 0) continue;
      const wx = x0 + x;
      if (wx < 0 || wx >= world.pxW) continue;
      buf[drow + wx] = v;
    }
  }
}

function drawPile(sim, buf, pile) {
  const world = sim.world;
  const res = RES[pile.res];
  const color = hexToPacked(res ? res.color : '#999999');
  const cx = world.anchorX(pile.col);
  const cy = world.anchorY(pile.row);
  const size = clamp(Math.round(Math.sqrt(pile.n) * 0.6), 1, Math.round(world.cellPx * 0.5));
  for (let y = -size; y <= 0; y++) {
    for (let x = -size; x <= size; x++) {
      if (Math.abs(x) + Math.abs(y) > size + 1) continue;
      const px = cx + x;
      const py = cy + y;
      if (px < 0 || px >= world.pxW || py < 0 || py >= world.pxH) continue;
      const n = hash2(px, py, pile.seed);
      buf[py * world.pxW + px] = mixPacked(color, packRGBA(0, 0, 0, 255), 0.25 * n);
    }
  }
}

function drawSmoke(sim, buf, b, sprite, sx, sy) {
  if (!sim.cfg.view.smoke || !b.def.smoke) return;
  if (sim.time - (b.active || -99) > 3) return;
  const world = sim.world;
  const top = sy - sprite.oy;
  const x = sx - sprite.ox + Math.round(sprite.w * 0.7);
  const puffs = b.def.smoke * 3;
  for (let i = 0; i < puffs; i++) {
    const phase = (sim.time * 6 + i * 3 + (b.seed % 7)) % 18;
    const py = Math.round(top - phase);
    const px = x + Math.round(Math.sin((phase + b.seed % 5) * 0.5) * 1.6);
    if (px < 0 || px >= world.pxW || py < 0 || py >= world.pxH) continue;
    const fade = 1 - phase / 18;
    const i2 = py * world.pxW + px;
    buf[i2] = mixPacked(buf[i2], packRGBA(210, 210, 205, 255), 0.5 * fade);
  }
}

// The ground under everything: terrain, worn paths and the contact shadows of
// whatever stands on it. Shadows are the expensive part of a frame, and they
// only change when a plant grows or a building goes up, so they are kept here
// and reused until something says otherwise.
function ensureGround(sim) {
  const buf = sim.buffer;
  const bgKey = `${sim.world.pxW}x${sim.world.pxH}:${sim.state.materials.version}:${sim.cfg.view.waterTop}:${sim.cfg.view.waterDeep}:${sim.terrainVersion || 0}`;
  if (!sim.bg || sim.bgKey !== bgKey || sim.bg.length !== buf.length) {
    sim.bg = new Uint32Array(buf.length);
    paintTerrain(sim, sim.bg);
    sim.bgKey = bgKey;
    sim.groundDirty = true;
  }
  if (!sim.ground || sim.ground.length !== buf.length) {
    sim.ground = new Uint32Array(buf.length);
    sim.groundDirty = true;
  }
  // Footpaths wear in and fade slowly, so a periodic rebuild is enough to keep
  // them current without paying for them every frame.
  sim.groundAge = (sim.groundAge || 0) + 1;
  if (!sim.groundDirty && sim.groundAge < 45) return;
  sim.groundAge = 0;
  sim.groundDirty = false;
  sim.ground.set(sim.bg);
  paintPaths(sim, sim.ground);
  if (sim.cfg.world.shadows !== false) {
    for (const plant of sim.plantSim.plants) {
      if (plant.species.sizeClass === 'ground' || plant.radiusPx <= 1) continue;
      sim.plantSim.castShadow(sim.ground, sim.world.anchorX(plant.col), sim.world.anchorY(plant.row), plant);
    }
    for (const b of sim.buildings) buildingShadow(sim, sim.ground, b);
  }
}

function buildingShadow(sim, buf, b) {
  const world = sim.world;
  const x0 = b.col * world.cellPx;
  const x1 = x0 + b.w * world.cellPx;
  const y1 = world.skyPx + (b.row + b.h) * world.depthPx;
  const drop = Math.max(1, Math.round(world.depthPx * 0.8));
  const dark = packRGBA(6, 10, 14, 255);
  for (let y = y1; y < y1 + drop; y++) {
    if (y < 0 || y >= world.pxH) continue;
    const t = (y - y1) / drop;
    for (let x = x0 - 1; x < x1 + 1; x++) {
      if (x < 0 || x >= world.pxW) continue;
      if (hash2(x, y, b.seed) < t * 0.9) continue;
      const i = y * world.pxW + x;
      buf[i] = mixPacked(buf[i], dark, 0.35 * (1 - t));
    }
  }
}

// One frame: the cached ground, then everything standing on it in back to
// front order. People move every frame, so this part runs every frame rather
// than only when something is marked dirty.
export function compositeSettlement(sim) {
  const world = sim.world;
  const buf = sim.buffer;
  ensureGround(sim);
  buf.set(sim.ground);

  const items = [];
  for (const plant of sim.plantSim.plants) {
    items.push({ row: plant.row, order: 1, id: plant.id, plant });
  }
  for (const pile of sim.piles) items.push({ row: pile.row, order: 0, id: pile.id, pile });
  for (const b of sim.buildings) {
    items.push({ row: b.row + b.h - 1, order: 2, id: b.id, building: b });
  }
  if (sim.cfg.view.people) {
    for (const p of sim.people) {
      items.push({ row: Math.floor(p.y), order: 3, id: p.id, person: p });
    }
  }
  items.sort((a, b) => (a.row !== b.row ? a.row - b.row : a.order !== b.order ? a.order - b.order : a.id - b.id));

  for (const item of items) {
    if (item.plant) {
      sim.plantSim.blitPlant(buf, item.plant, false);
    } else if (item.pile) {
      drawPile(sim, buf, item.pile);
    } else if (item.building) {
      const b = item.building;
      const sprite = buildingSprite(sim, b);
      const sx = b.col * world.cellPx;
      const sy = world.skyPx + (b.row + b.h) * world.depthPx;
      blit(buf, world, sprite, sx + sprite.ox, sy);
      drawSmoke(sim, buf, b, sprite, sx + sprite.ox, sy);
    } else if (item.person) {
      const p = item.person;
      const frame = Math.floor(p.bob) % 2;
      const sprite = personSprite(sim, p, p.path ? frame : 0);
      const sx = Math.round(p.x * world.cellPx);
      const sy = Math.round(world.skyPx + p.y * world.depthPx);
      blit(buf, world, sprite, sx, sy);
      if (p.carrying) drawCarry(sim, buf, p, sprite, sx, sy);
    }
  }
  sim.bufferDirty = false;
  return buf;
}

function drawCarry(sim, buf, p, sprite, sx, sy) {
  const world = sim.world;
  const res = RES[p.carry.res];
  if (!res) return;
  const color = hexToPacked(res.color);
  const size = clamp(Math.round(sprite.w * 0.4), 1, 3);
  const x0 = sx + (p.facing > 0 ? sprite.ox : -sprite.ox - size + 1);
  const y0 = sy - sprite.oy + Math.round(sprite.h * 0.35);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const px = x0 + x;
      const py = y0 + y;
      if (px < 0 || px >= world.pxW || py < 0 || py >= world.pxH) continue;
      buf[py * world.pxW + px] = y === 0 ? mixPacked(color, packRGBA(255, 255, 255, 255), 0.25) : color;
    }
  }
}

// The night tint and the debug overlays are drawn on the canvas rather than
// into the pixel buffer: darkening 300k pixels per frame by hand is not worth
// it when one translucent rectangle does the same job.
export function drawCivOverlay(sim, ctx, viewport) {
  const world = sim.world;
  const zoom = viewport.zoom;
  const w = world.pxW * zoom;
  const h = world.pxH * zoom;
  if (sim.cfg.view.dayNight) {
    const light = sim.daylight;
    if (light < 0.95) {
      const dark = (1 - light) * 0.55;
      const tint = unpackRGBA(hexToPacked(sim.cfg.world.skyTop));
      ctx.fillStyle = `rgba(${tint.r}, ${tint.g}, ${tint.b}, ${dark.toFixed(3)})`;
      ctx.fillRect(viewport.panX, viewport.panY, w, h);
    }
  }
  if (sim.cfg.view.labels) drawLabels(sim, ctx, viewport);
}

function drawLabels(sim, ctx, viewport) {
  const world = sim.world;
  const zoom = viewport.zoom;
  ctx.font = `${Math.max(9, Math.round(7 * Math.min(3, zoom)))}px ui-monospace, monospace`;
  ctx.textAlign = 'center';
  ctx.fillStyle = 'rgba(230, 236, 245, 0.85)';
  ctx.strokeStyle = 'rgba(6, 10, 16, 0.9)';
  ctx.lineWidth = 3;
  for (const b of sim.buildings) {
    const x = viewport.panX + (b.col * world.cellPx + (b.w * world.cellPx) / 2) * zoom;
    const y = viewport.panY + (world.skyPx + (b.row + b.h) * world.depthPx) * zoom + 10;
    const text = b.built ? b.def.label : `${b.def.label} ${Math.round((b.workDone / b.work) * 100)}%`;
    ctx.strokeText(text, x, y);
    ctx.fillText(text, x, y);
  }
}
