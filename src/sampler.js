// Sampling boxes.
//
// A sampler is a small drawable pixel grid that materials are sampled from.
// Two layouts are supported and can be switched at any time:
//
//   mode 'multi'  - every sampler owns its own grid.
//   mode 'single' - all samplers read from one shared atlas grid; each sampler
//                   owns a rectangular region of it.
//
// A sampler is read as a ramp: its unique colors sorted dark to light, indexed
// by a tone value. How the colors are arranged in the grid does not matter,
// only which distinct colors are present.

import {
  EMPTY_COLOR,
  hslToPacked,
  luminance,
  packedToRGBAHex,
  rgbaHexToPacked,
} from './util.js';

export const ROLES = [
  { id: 'ground', label: 'Ground cover', hue: 96, sat: 0.28, l0: 0.16, l1: 0.5 },
  { id: 'soil', label: 'Soil', hue: 24, sat: 0.3, l0: 0.09, l1: 0.4 },
  { id: 'trunk', label: 'Tree base / trunk', hue: 26, sat: 0.34, l0: 0.14, l1: 0.5 },
  { id: 'branch', label: 'Branches', hue: 32, sat: 0.3, l0: 0.16, l1: 0.54 },
  { id: 'leaf', label: 'Leaf texture', hue: 118, sat: 0.42, l0: 0.14, l1: 0.56 },
  { id: 'leafEdge', label: 'Leaf edges', hue: 82, sat: 0.5, l0: 0.2, l1: 0.66 },
  { id: 'stem', label: 'Stem to leaf', hue: 74, sat: 0.38, l0: 0.16, l1: 0.5 },
  { id: 'stone', label: 'Stone', hue: 210, sat: 0.06, l0: 0.18, l1: 0.58 },
  { id: 'timber', label: 'Timber wall', hue: 30, sat: 0.3, l0: 0.16, l1: 0.52 },
  { id: 'plank', label: 'Sawn plank', hue: 38, sat: 0.34, l0: 0.22, l1: 0.66 },
  { id: 'thatch', label: 'Thatch roof', hue: 46, sat: 0.42, l0: 0.2, l1: 0.62 },
  { id: 'brick', label: 'Brick', hue: 14, sat: 0.44, l0: 0.18, l1: 0.56 },
  { id: 'metal', label: 'Metal', hue: 205, sat: 0.1, l0: 0.24, l1: 0.74 },
  { id: 'cloth', label: 'Cloth', hue: 330, sat: 0.26, l0: 0.24, l1: 0.68 },
];

export const ROLE_LABELS = Object.fromEntries(ROLES.map((r) => [r.id, r.label]));

let rampCache = new Map();

export function invalidateSamplerCache() {
  rampCache = new Map();
}

export function createSampler({ id, name, role, w = 16, h = 8, region = null }) {
  return {
    id,
    name,
    role,
    w,
    h,
    px: new Uint32Array(w * h),
    region: region || { x: 0, y: 0, w, h },
  };
}

// Fills a sampler with a plausible starting ramp so the tool is usable before
// anything has been drawn. The lightness sweep is snapped to a small number of
// steps, so the box reads as pixel art and the resolved ramp stays short
// instead of holding one unique color per pixel.
export const DEFAULT_TONES = 6;

export function fillDefaultArt(sampler, roleDef, seedOffset = 0, tones = DEFAULT_TONES) {
  const { w, h, px } = sampler;
  const steps = Math.max(2, Math.min(tones, w * h));
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const u = w > 1 ? x / (w - 1) : 0;
      const v = h > 1 ? y / (h - 1) : 0;
      const dither = (((x * 7 + y * 13 + seedOffset) % 3) - 1) * 0.06;
      const t = Math.min(1, Math.max(0, u + dither + (v - 0.5) * 0.08));
      const idx = Math.round(t * (steps - 1));
      const f = idx / (steps - 1);
      const l = roleDef.l0 + (roleDef.l1 - roleDef.l0) * f;
      const hue = roleDef.hue + (f - 0.5) * 10;
      const sat = roleDef.sat * (1 - f * 0.12);
      px[y * w + x] = hslToPacked(hue, sat, l);
    }
  }
}

export function resizeSampler(sampler, w, h) {
  const next = new Uint32Array(w * h);
  for (let y = 0; y < h; y++) {
    const sy = sampler.h > 1 ? Math.min(sampler.h - 1, Math.floor((y * sampler.h) / h)) : 0;
    for (let x = 0; x < w; x++) {
      const sx = sampler.w > 1 ? Math.min(sampler.w - 1, Math.floor((x * sampler.w) / w)) : 0;
      next[y * w + x] = sampler.px[sy * sampler.w + sx];
    }
  }
  sampler.px = next;
  sampler.w = w;
  sampler.h = h;
  invalidateSamplerCache();
}

export function createMaterials() {
  // The shared grid is sized so every role gets a band of equal height with no
  // leftover rows.
  const bandH = 3;
  const atlasW = 24;
  const atlasH = bandH * ROLES.length;
  const materials = {
    mode: 'multi',
    atlas: { w: atlasW, h: atlasH, px: new Uint32Array(atlasW * atlasH) },
    samplers: [],
    version: 1,
  };
  ROLES.forEach((role, i) => {
    const s = createSampler({
      id: `mat-${role.id}`,
      name: role.label,
      role: role.id,
      w: 16,
      h: 6,
      region: { x: 0, y: i * bandH, w: materials.atlas.w, h: bandH },
    });
    fillDefaultArt(s, role, i * 31);
    materials.samplers.push(s);
  });
  paintAtlasFromSamplers(materials);
  return materials;
}

// Copies each sampler's own art into its atlas region, so switching to single
// grid mode starts from what the separate boxes already show.
// A project saved before a role existed has no sampler for it. Rather than
// leaving that material unpainted, the missing boxes are appended with their
// default art and the shared atlas is grown to fit their bands.
export function ensureRoleSamplers(materials) {
  const bandH = 3;
  const have = new Set(materials.samplers.map((s) => s.role));
  const missing = ROLES.filter((r) => !have.has(r.id));
  if (missing.length === 0) return materials;
  const neededH = ROLES.length * bandH;
  if (materials.atlas.h < neededH) {
    const px = new Uint32Array(materials.atlas.w * neededH);
    px.set(materials.atlas.px.subarray(0, Math.min(materials.atlas.px.length, px.length)));
    materials.atlas = { w: materials.atlas.w, h: neededH, px };
  }
  for (const role of missing) {
    const index = ROLES.findIndex((r) => r.id === role.id);
    const s = createSampler({
      id: `mat-${role.id}`,
      name: role.label,
      role: role.id,
      w: 16,
      h: 6,
      region: { x: 0, y: index * bandH, w: materials.atlas.w, h: bandH },
    });
    fillDefaultArt(s, role, index * 31);
    materials.samplers.push(s);
  }
  materials.version++;
  invalidateSamplerCache();
  return materials;
}

export function paintAtlasFromSamplers(materials) {
  const { atlas } = materials;
  atlas.px.fill(EMPTY_COLOR);
  for (const s of materials.samplers) {
    const r = s.region;
    for (let y = 0; y < r.h; y++) {
      const ay = r.y + y;
      if (ay < 0 || ay >= atlas.h) continue;
      const sy = s.h > 1 ? Math.min(s.h - 1, Math.floor((y * s.h) / Math.max(1, r.h))) : 0;
      for (let x = 0; x < r.w; x++) {
        const ax = r.x + x;
        if (ax < 0 || ax >= atlas.w) continue;
        const sx = s.w > 1 ? Math.min(s.w - 1, Math.floor((x * s.w) / Math.max(1, r.w))) : 0;
        atlas.px[ay * atlas.w + ax] = s.px[sy * s.w + sx];
      }
    }
  }
  materials.version++;
  invalidateSamplerCache();
}

export function copyAtlasToSamplers(materials) {
  const { atlas } = materials;
  for (const s of materials.samplers) {
    const r = s.region;
    resizeSampler(s, Math.max(1, r.w), Math.max(1, r.h));
    for (let y = 0; y < r.h; y++) {
      for (let x = 0; x < r.w; x++) {
        const ax = r.x + x;
        const ay = r.y + y;
        s.px[y * s.w + x] =
          ax >= 0 && ax < atlas.w && ay >= 0 && ay < atlas.h ? atlas.px[ay * atlas.w + ax] : EMPTY_COLOR;
      }
    }
  }
  materials.version++;
  invalidateSamplerCache();
}

export function findSampler(materials, id) {
  return materials.samplers.find((s) => s.id === id) || null;
}

// The pixel buffer a sampler currently reads from, honoring the active mode.
export function samplerPatch(materials, sampler) {
  if (!sampler) return { w: 1, h: 1, px: new Uint32Array(1) };
  if (materials.mode === 'multi') return { w: sampler.w, h: sampler.h, px: sampler.px };
  const { atlas } = materials;
  const r = sampler.region;
  const w = Math.max(1, Math.min(r.w, atlas.w - r.x));
  const h = Math.max(1, Math.min(r.h, atlas.h - r.y));
  const px = new Uint32Array(w * h);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) px[y * w + x] = atlas.px[(r.y + y) * atlas.w + (r.x + x)];
  }
  return { w, h, px };
}

// Unique opaque colors of a sampler, sorted dark to light. Cached per
// materials version so the sim can call this every pixel.
export function samplerRamp(materials, sampler) {
  if (!sampler) return [];
  const key = `${materials.version}:${materials.mode}:${sampler.id}`;
  const hit = rampCache.get(key);
  if (hit) return hit;
  const patch = samplerPatch(materials, sampler);
  const seen = new Set();
  const colors = [];
  for (let i = 0; i < patch.px.length; i++) {
    const v = patch.px[i];
    if (v === EMPTY_COLOR) continue;
    if (seen.has(v)) continue;
    seen.add(v);
    colors.push(v);
  }
  colors.sort((a, b) => luminance(a) - luminance(b));
  rampCache.set(key, colors);
  return colors;
}

export function rampPick(ramp, t) {
  if (ramp.length === 0) return EMPTY_COLOR;
  const i = Math.round(t * (ramp.length - 1));
  return ramp[i < 0 ? 0 : i >= ramp.length ? ramp.length - 1 : i];
}

export function serializeMaterials(materials) {
  return {
    mode: materials.mode,
    atlas: {
      w: materials.atlas.w,
      h: materials.atlas.h,
      px: encodePixels(materials.atlas.px),
    },
    samplers: materials.samplers.map((s) => ({
      id: s.id,
      name: s.name,
      role: s.role,
      w: s.w,
      h: s.h,
      region: { ...s.region },
      px: encodePixels(s.px),
    })),
  };
}

export function deserializeMaterials(data) {
  const materials = {
    mode: data.mode === 'single' ? 'single' : 'multi',
    atlas: {
      w: data.atlas.w,
      h: data.atlas.h,
      px: decodePixels(data.atlas.px, data.atlas.w * data.atlas.h),
    },
    samplers: data.samplers.map((s) => ({
      id: s.id,
      name: s.name,
      role: s.role,
      w: s.w,
      h: s.h,
      region: { ...s.region },
      px: decodePixels(s.px, s.w * s.h),
    })),
    version: 1,
  };
  invalidateSamplerCache();
  return ensureRoleSamplers(materials);
}

function encodePixels(px) {
  let out = '';
  for (let i = 0; i < px.length; i++) out += packedToRGBAHex(px[i]);
  return out;
}

function decodePixels(str, count) {
  const px = new Uint32Array(count);
  for (let i = 0; i < count; i++) {
    const chunk = str.slice(i * 8, i * 8 + 8);
    px[i] = chunk.length === 8 ? rgbaHexToPacked(chunk) : EMPTY_COLOR;
  }
  return px;
}
