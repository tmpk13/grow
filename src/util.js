// Small numeric / color / raster helpers shared by the whole tool.
// Colors are stored packed in a Uint32 using the native byte order of an
// ImageData Uint32 view, so sprites can be blitted with a plain array copy.

const LITTLE_ENDIAN = (() => {
  const buf = new ArrayBuffer(4);
  new Uint32Array(buf)[0] = 0x11223344;
  return new Uint8Array(buf)[0] === 0x44;
})();

export const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);
export const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
export const lerp = (a, b, t) => a + (b - a) * t;
export const smoothstep = (t) => t * t * (3 - 2 * t);
export const toRad = (deg) => (deg * Math.PI) / 180;

export const packRGBA = LITTLE_ENDIAN
  ? (r, g, b, a) => (((a & 255) << 24) | ((b & 255) << 16) | ((g & 255) << 8) | (r & 255)) >>> 0
  : (r, g, b, a) => (((r & 255) << 24) | ((g & 255) << 16) | ((b & 255) << 8) | (a & 255)) >>> 0;

export const unpackRGBA = LITTLE_ENDIAN
  ? (v) => ({ r: v & 255, g: (v >>> 8) & 255, b: (v >>> 16) & 255, a: (v >>> 24) & 255 })
  : (v) => ({ r: (v >>> 24) & 255, g: (v >>> 16) & 255, b: (v >>> 8) & 255, a: v & 255 });

export const EMPTY_COLOR = 0;

export function hexToPacked(hex, alpha = 255) {
  let s = String(hex).replace('#', '').trim();
  if (s.length === 3) s = s[0] + s[0] + s[1] + s[1] + s[2] + s[2];
  const r = parseInt(s.slice(0, 2), 16) || 0;
  const g = parseInt(s.slice(2, 4), 16) || 0;
  const b = parseInt(s.slice(4, 6), 16) || 0;
  const a = s.length >= 8 ? parseInt(s.slice(6, 8), 16) : alpha;
  return packRGBA(r, g, b, a);
}

const hex2 = (n) => n.toString(16).padStart(2, '0');

export function packedToHex(v) {
  const c = unpackRGBA(v);
  return `#${hex2(c.r)}${hex2(c.g)}${hex2(c.b)}`;
}

export function packedToRGBAHex(v) {
  const c = unpackRGBA(v);
  return `${hex2(c.r)}${hex2(c.g)}${hex2(c.b)}${hex2(c.a)}`;
}

export function rgbaHexToPacked(s) {
  const r = parseInt(s.slice(0, 2), 16) || 0;
  const g = parseInt(s.slice(2, 4), 16) || 0;
  const b = parseInt(s.slice(4, 6), 16) || 0;
  const a = parseInt(s.slice(6, 8), 16) || 0;
  return packRGBA(r, g, b, a);
}

export function luminance(v) {
  const c = unpackRGBA(v);
  return 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
}

export function mixPacked(a, b, t) {
  const ca = unpackRGBA(a);
  const cb = unpackRGBA(b);
  return packRGBA(
    Math.round(lerp(ca.r, cb.r, t)),
    Math.round(lerp(ca.g, cb.g, t)),
    Math.round(lerp(ca.b, cb.b, t)),
    Math.round(lerp(ca.a, cb.a, t)),
  );
}

export function hslToPacked(h, s, l, a = 255) {
  const hh = ((h % 360) + 360) % 360 / 360;
  const ss = clamp01(s);
  const ll = clamp01(l);
  const q = ll < 0.5 ? ll * (1 + ss) : ll + ss - ll * ss;
  const p = 2 * ll - q;
  const chan = (t) => {
    let x = t;
    if (x < 0) x += 1;
    if (x > 1) x -= 1;
    if (x < 1 / 6) return p + (q - p) * 6 * x;
    if (x < 1 / 2) return q;
    if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
    return p;
  };
  return packRGBA(
    Math.round(chan(hh + 1 / 3) * 255),
    Math.round(chan(hh) * 255),
    Math.round(chan(hh - 1 / 3) * 255),
    a,
  );
}

// Stable value noise in [0,1). Used for blob shapes and per-pixel jitter so
// that a re-raster of the same plant produces identical pixels.
export function hash2(x, y, seed) {
  let h = (x | 0) * 374761393 + (y | 0) * 668265263 + (seed | 0) * 1442695041;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

// Chamfer 3-4 distance transform: distance (in pixels) from every non-zero
// mask pixel to the nearest zero pixel. Out of bounds counts as zero, so a
// shape touching the buffer edge is treated as ending there.
export function distanceTransform(mask, w, h, out) {
  const d = out && out.length === w * h ? out : new Float32Array(w * h);
  const INF = 1e9;
  for (let i = 0; i < w * h; i++) d[i] = mask[i] ? INF : 0;
  const A = 3;
  const B = 4;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      if (d[i] === 0) continue;
      let best = d[i];
      if (y > 0) {
        if (x > 0) best = Math.min(best, d[i - w - 1] + B);
        best = Math.min(best, d[i - w] + A);
        if (x < w - 1) best = Math.min(best, d[i - w + 1] + B);
      } else {
        best = Math.min(best, A);
      }
      if (x > 0) best = Math.min(best, d[i - 1] + A);
      else best = Math.min(best, A);
      if (x === w - 1) best = Math.min(best, A);
      d[i] = best;
    }
  }
  for (let y = h - 1; y >= 0; y--) {
    for (let x = w - 1; x >= 0; x--) {
      const i = y * w + x;
      if (d[i] === 0) continue;
      let best = d[i];
      if (y < h - 1) {
        if (x < w - 1) best = Math.min(best, d[i + w + 1] + B);
        best = Math.min(best, d[i + w] + A);
        if (x > 0) best = Math.min(best, d[i + w - 1] + B);
      } else {
        best = Math.min(best, A);
      }
      if (x < w - 1) best = Math.min(best, d[i + 1] + A);
      else best = Math.min(best, A);
      d[i] = best;
    }
  }
  for (let i = 0; i < d.length; i++) d[i] /= 3;
  return d;
}

// 4-connected labelling. Returns labels (-1 for background) plus a bounding
// box per component; the caller fills in maxDepth from the distance transform.
export function labelComponents(mask, w, h, labels, stack) {
  const lab = labels && labels.length === w * h ? labels : new Int32Array(w * h);
  lab.fill(-1);
  const comps = [];
  const st = stack && stack.length >= w * h ? stack : new Int32Array(w * h);
  for (let seed = 0; seed < w * h; seed++) {
    if (!mask[seed] || lab[seed] !== -1) continue;
    const id = comps.length;
    const comp = { x0: w, y0: h, x1: 0, y1: 0, maxDepth: 0, count: 0 };
    comps.push(comp);
    let sp = 0;
    st[sp++] = seed;
    lab[seed] = id;
    while (sp > 0) {
      const i = st[--sp];
      const x = i % w;
      const y = (i / w) | 0;
      if (x < comp.x0) comp.x0 = x;
      if (x > comp.x1) comp.x1 = x;
      if (y < comp.y0) comp.y0 = y;
      if (y > comp.y1) comp.y1 = y;
      comp.count++;
      if (x > 0 && mask[i - 1] && lab[i - 1] === -1) { lab[i - 1] = id; st[sp++] = i - 1; }
      if (x < w - 1 && mask[i + 1] && lab[i + 1] === -1) { lab[i + 1] = id; st[sp++] = i + 1; }
      if (y > 0 && mask[i - w] && lab[i - w] === -1) { lab[i - w] = id; st[sp++] = i - w; }
      if (y < h - 1 && mask[i + w] && lab[i + w] === -1) { lab[i + w] = id; st[sp++] = i + w; }
    }
  }
  return { labels: lab, comps };
}

export function uid(prefix = 'id') {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`;
}

export function deepClone(obj) {
  return JSON.parse(JSON.stringify(obj));
}
