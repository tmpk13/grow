// A growing plant instance.
//
// Growth happens in the plant's own pixel space (a sprite buffer anchored at
// the root pixel, bottom center). Each growth step advances one active tip,
// which may branch, droop, climb a support or terminate into a leaf cluster.
//
// Rendering is a two stage process:
//   1. rasterize segments and leaf blobs into a material id mask
//   2. shade every pixel from its depth inside its own shape and its vertical
//      position inside that shape, then look the tone up in the sampling box
//      assigned to that material
//
// Step 2 treats trunk, branch and stem as one body and leaf plus leaf edge as
// another, so a leaf is shaded as a leaf and not as part of the branch it
// hangs off.

import {
  clamp,
  clamp01,
  distanceTransform,
  hash2,
  labelComponents,
  toRad,
} from './util.js';
import { quantize, shadeValue } from './shading.js';
import { rampPick } from './sampler.js';

export const MAT = {
  EMPTY: 0,
  TRUNK: 1,
  BRANCH: 2,
  LEAF: 3,
  LEAF_EDGE: 4,
  STEM: 5,
  GROUND: 6,
};

export const MAT_SLOT = {
  [MAT.TRUNK]: 'trunk',
  [MAT.BRANCH]: 'branch',
  [MAT.LEAF]: 'leaf',
  [MAT.LEAF_EDGE]: 'leafEdge',
  [MAT.STEM]: 'stem',
  [MAT.GROUND]: 'ground',
};

// Materials shaded together as one body.
const SHADE_GROUPS = [
  { mats: [MAT.TRUNK, MAT.BRANCH, MAT.STEM], core: 'coreWood' },
  { mats: [MAT.LEAF, MAT.LEAF_EDGE], core: 'coreLeaf' },
  { mats: [MAT.GROUND], core: 'coreWood' },
];

const SPRITE_PAD = 4;

function angleDiff(target, current) {
  let d = target - current;
  while (d > Math.PI) d -= 2 * Math.PI;
  while (d < -Math.PI) d += 2 * Math.PI;
  return d;
}

export class Plant {
  constructor({ id, species, limits, col, row, world, rng }) {
    this.id = id;
    this.species = species;
    this.limits = limits;
    this.col = col;
    this.row = row;
    this.layer = null; // assigned by the sim
    this.rng = rng;
    this.seed = rng.seed();
    this.age = 0;
    this.alive = true;
    this.budget = 0;
    this.depthShade = 0; // atmospheric lift for far rows, set by the sim
    this.growthRate = rng.range(species.growth.rateMin, species.growth.rateMax);

    const cellPx = world.cellPx;
    this.cellPx = cellPx;
    this.depthRatio = world.depthRatio || 1;
    const maxRadiusPx = limits.maxRadiusCells * cellPx + cellPx / 2;
    this.w = Math.ceil(maxRadiusPx * 2 + SPRITE_PAD * 2);
    this.ox = Math.floor(this.w / 2);
    this.maxRadiusPx = maxRadiusPx;
    if (species.sizeClass === 'ground') {
      // A mat lies flat on the ground plane, so its sprite is a foreshortened
      // disc centered on the anchor instead of a shape standing on it.
      this.maxRadiusYPx = Math.max(1, maxRadiusPx * this.depthRatio);
      this.h = Math.ceil(this.maxRadiusYPx * 2 + limits.maxHeightPx + SPRITE_PAD * 2);
      this.oy = Math.round(SPRITE_PAD + this.maxRadiusYPx);
    } else {
      this.maxRadiusYPx = 0;
      this.h = Math.ceil(limits.maxHeightPx + SPRITE_PAD * 2);
      this.oy = this.h - SPRITE_PAD;
    }

    this.segments = [];
    this.leaves = [];
    this.tips = [];
    this.grantedRadiusCells = 0;
    this.cells = [];
    this.confinedSide = false;
    this.radiusPx = 0;
    this.heightPx = 0;

    this.mask = new Uint8Array(this.w * this.h);
    this.bias = new Int8Array(this.w * this.h);
    this.sprite = new Uint32Array(this.w * this.h);
    this.bounds = { x0: 0, y0: 0, x1: -1, y1: -1 };
    this.dirty = true;
    this.scratch = null;

    this.initTips();
  }

  initTips() {
    const f = this.species.form;
    if (this.species.sizeClass === 'ground') return;
    this.tips.push({
      x: this.ox,
      y: this.oy,
      angle: -Math.PI / 2 + toRad(this.rng.range(-6, 6)),
      width: f.baseWidth,
      depth: 0,
      len: 0,
      sinceBranch: 0,
      phase: this.rng.range(0, Math.PI * 2),
      dir: this.rng.sign(),
      support: null,
      alive: true,
    });
  }

  get aliveTipCount() {
    let n = 0;
    for (const t of this.tips) if (t.alive) n++;
    return n;
  }

  get mature() {
    if (this.species.sizeClass !== 'ground') return this.aliveTipCount === 0;
    const spread = this.radiusPx >= this.maxRadiusPx || this.confinedSide;
    return spread && this.heightPx >= this.limits.maxHeightPx;
  }

  grow(dt, ctx) {
    if (!this.alive) return;
    this.age += dt;
    if (this.age > this.species.growth.maxAge) {
      this.alive = false;
      return;
    }
    if (this.mature) return;
    this.budget += this.growthRate * dt;
    let guard = 0;
    while (this.budget >= 1 && guard < 64) {
      this.budget -= 1;
      guard++;
      if (this.species.sizeClass === 'ground') this.stepGround(ctx);
      else this.stepBranching(ctx);
      if (this.mature) break;
    }
  }

  // A patch thickens whether or not it can still spread sideways, so a mat
  // hemmed in by neighbors still fills out instead of staying one pixel tall.
  stepGround(ctx) {
    if (this.heightPx < this.limits.maxHeightPx) {
      this.heightPx = Math.min(this.limits.maxHeightPx, this.heightPx + 0.75);
      this.dirty = true;
    }
    if (this.radiusPx >= this.maxRadiusPx) return;
    const next = this.radiusPx + 1;
    const cells = Math.ceil(next / this.cellPx);
    if (cells > this.grantedRadiusCells && !this.requestSpace(ctx, cells)) {
      this.confinedSide = true;
      return;
    }
    this.radiusPx = next;
    this.dirty = true;
  }

  stepBranching(ctx) {
    const alive = [];
    for (let i = 0; i < this.tips.length; i++) if (this.tips[i].alive) alive.push(i);
    if (alive.length === 0) return;
    const tip = this.tips[alive[Math.floor(this.rng.next() * alive.length) % alive.length]];
    this.advanceTip(tip, ctx);
    this.dirty = true;
  }

  advanceTip(tip, ctx) {
    const sp = this.species;
    const f = sp.form;
    const rng = this.rng;

    tip.angle += toRad(rng.range(-f.wander, f.wander)) * 0.5;
    tip.angle += angleDiff(-Math.PI / 2, tip.angle) * f.phototropism * 0.3;
    if (tip.depth > 0) {
      tip.angle += angleDiff(Math.PI / 2, tip.angle) * f.gravity * 0.12 * tip.depth;
    }

    let behind = false;
    if (f.wrap) behind = this.steerClimb(tip, ctx, f);

    const step = rng.range(sp.growth.stepMin, sp.growth.stepMax);
    let nx = tip.x + Math.cos(tip.angle) * step;
    let ny = tip.y + Math.sin(tip.angle) * step;

    // Spreading wider needs ground cells; when the world will not grant them
    // the tip is steered back inward instead of stopping dead, so a crowded
    // plant grows tall and narrow.
    const wantRadius = Math.abs(nx - this.ox);
    if (wantRadius > this.grantedRadiusCells * this.cellPx + this.cellPx / 2) {
      const cells = Math.min(this.limits.maxRadiusCells, Math.ceil(wantRadius / this.cellPx));
      if (cells > this.grantedRadiusCells && !this.requestSpace(ctx, cells)) {
        this.confinedSide = true;
        tip.angle += angleDiff(-Math.PI / 2, tip.angle) * 0.6;
        nx = tip.x + Math.cos(tip.angle) * step;
        ny = tip.y + Math.sin(tip.angle) * step;
      }
    }

    const limitX = this.maxRadiusPx;
    if (Math.abs(nx - this.ox) > limitX || ny < this.oy - this.limits.maxHeightPx || ny > this.oy + 2) {
      this.endTip(tip);
      return;
    }

    const width = Math.max(f.minWidth, tip.width);
    const mat = tip.depth === 0 ? MAT.TRUNK : MAT.BRANCH;
    this.segments.push({
      x0: tip.x,
      y0: tip.y,
      x1: nx,
      y1: ny,
      w: width,
      mat,
      bias: behind ? -Math.round(sp.shade.behindShade * 100) : 0,
    });

    tip.x = nx;
    tip.y = ny;
    tip.len += step;
    tip.sinceBranch += step;
    tip.width *= f.taper;
    this.radiusPx = Math.max(this.radiusPx, Math.abs(nx - this.ox));
    this.heightPx = Math.max(this.heightPx, this.oy - ny);

    if (tip.depth >= f.leafDepth && rng.chance(f.leafDensity)) this.addLeaf(tip, behind);

    if (
      tip.sinceBranch >= f.branchInterval &&
      tip.depth < f.maxDepth &&
      this.aliveTipCount < this.limits.maxTips &&
      rng.chance(f.branchChance)
    ) {
      this.branch(tip, f);
    }

    if (tip.width < f.minWidth || tip.len > this.limits.maxHeightPx * 1.6) {
      this.endTip(tip);
    }
  }

  // Vines look for a woody neighbor anywhere in the surrounding area and coil
  // up it; with nothing to climb they creep sideways along the ground instead.
  steerClimb(tip, ctx, f) {
    if (!tip.support && ctx && ctx.world) {
      const search = Math.min(f.climbSearch, this.limits.maxRadiusCells);
      const found = ctx.world.findSupport(this.col, this.row, search, ctx.supportLayers);
      if (found && found.owner !== this.id) tip.support = found;
    }
    if (!tip.support) {
      const target = tip.dir > 0 ? 0 : Math.PI;
      tip.angle += angleDiff(target, tip.angle) * 0.35;
      return false;
    }
    tip.phase += f.wrapPitch;
    // Supports are found anywhere in the area but climbed on screen, so only
    // the horizontal offset steers the tip; the depth offset is left alone.
    const targetX = this.ox + (tip.support.col - this.col) * this.cellPx;
    const pull = clamp((targetX - tip.x) * 0.05, -0.7, 0.7);
    const sway = Math.sin(tip.phase) * toRad(f.wrapAmp);
    const desired = -Math.PI / 2 + sway + pull;
    tip.angle += angleDiff(desired, tip.angle) * 0.55;
    return Math.cos(tip.phase) < 0;
  }

  branch(tip, f) {
    const rng = this.rng;
    const side = rng.sign();
    const angle = toRad(rng.range(f.branchAngleMin, f.branchAngleMax)) * side;
    this.tips.push({
      x: tip.x,
      y: tip.y,
      angle: tip.angle + angle,
      width: Math.max(f.minWidth, tip.width * 0.72),
      depth: tip.depth + 1,
      len: 0,
      sinceBranch: 0,
      phase: tip.phase + Math.PI * 0.5,
      dir: -tip.dir,
      support: tip.support,
      alive: true,
    });
    tip.angle -= angle * 0.35;
    tip.sinceBranch = 0;
    tip.width *= 0.94;
  }

  endTip(tip) {
    if (!tip.alive) return;
    tip.alive = false;
    const f = this.species.form;
    if (f.leafDensity > 0 && tip.depth >= f.leafDepth) this.addLeaf(tip, false);
  }

  addLeaf(tip, behind) {
    const f = this.species.form;
    const rng = this.rng;
    const r = rng.range(f.leafSizeMin, f.leafSizeMax);
    const side = rng.sign();
    const off = f.petiole + r * 0.5;
    const a = tip.angle + side * toRad(rng.range(30, 80));
    const lx = tip.x + Math.cos(a) * off;
    const ly = tip.y + Math.sin(a) * off;
    if (Math.abs(lx - this.ox) > this.maxRadiusPx - r || ly < r || ly > this.oy) return;
    if (f.petiole > 0) {
      this.segments.push({
        x0: tip.x,
        y0: tip.y,
        x1: lx - Math.cos(a) * r * 0.4,
        y1: ly - Math.sin(a) * r * 0.4,
        w: 1,
        mat: MAT.STEM,
        bias: behind ? -Math.round(this.species.shade.behindShade * 100) : 0,
      });
    }
    this.leaves.push({
      x: lx,
      y: ly,
      rx: r * rng.range(0.9, 1.35),
      ry: r * rng.range(0.7, 1.1),
      seed: rng.seed(),
      bias: behind ? -Math.round(this.species.shade.behindShade * 100) : 0,
    });
    this.radiusPx = Math.max(this.radiusPx, Math.abs(lx - this.ox) + r);
    this.heightPx = Math.max(this.heightPx, this.oy - (ly - r));
  }

  // Asks the sim for a larger footprint on the ground plane. Returns false
  // when a neighbor of the same size class already owns one of the cells.
  requestSpace(ctx, radiusCells) {
    if (!ctx || !ctx.requestSpace) {
      this.grantedRadiusCells = Math.max(this.grantedRadiusCells, radiusCells);
      return true;
    }
    const ok = ctx.requestSpace(this, radiusCells);
    if (ok) this.grantedRadiusCells = Math.max(this.grantedRadiusCells, radiusCells);
    return ok;
  }

  // ---- rasterizing -------------------------------------------------------

  stampDisc(cx, cy, r, mat, bias) {
    const { mask, bias: biasBuf, w, h } = this;
    const rr = Math.max(0.5, r);
    const x0 = Math.max(0, Math.floor(cx - rr));
    const x1 = Math.min(w - 1, Math.ceil(cx + rr));
    const y0 = Math.max(0, Math.floor(cy - rr));
    const y1 = Math.min(h - 1, Math.ceil(cy + rr));
    const r2 = rr * rr;
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        const dx = x + 0.5 - cx;
        const dy = y + 0.5 - cy;
        if (dx * dx + dy * dy > r2) continue;
        const i = y * w + x;
        mask[i] = mat;
        biasBuf[i] = bias;
      }
    }
  }

  stampSegment(seg) {
    const dx = seg.x1 - seg.x0;
    const dy = seg.y1 - seg.y0;
    const len = Math.hypot(dx, dy);
    const steps = Math.max(1, Math.ceil(len * 2));
    for (let i = 0; i <= steps; i++) {
      const t = i / steps;
      this.stampDisc(seg.x0 + dx * t, seg.y0 + dy * t, seg.w / 2, seg.mat, seg.bias);
    }
  }

  stampLeaf(leaf) {
    const { mask, bias: biasBuf, w, h } = this;
    const x0 = Math.max(0, Math.floor(leaf.x - leaf.rx - 1));
    const x1 = Math.min(w - 1, Math.ceil(leaf.x + leaf.rx + 1));
    const y0 = Math.max(0, Math.floor(leaf.y - leaf.ry - 1));
    const y1 = Math.min(h - 1, Math.ceil(leaf.y + leaf.ry + 1));
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        const dx = (x + 0.5 - leaf.x) / Math.max(0.5, leaf.rx);
        const dy = (y + 0.5 - leaf.y) / Math.max(0.5, leaf.ry);
        const d = Math.sqrt(dx * dx + dy * dy);
        const wobble = (hash2(x, y, leaf.seed) - 0.5) * 0.45;
        if (d > 1 + wobble) continue;
        const i = y * w + x;
        mask[i] = MAT.LEAF;
        biasBuf[i] = leaf.bias;
      }
    }
  }

  // A mat is a ragged disc lying on the ground plane, squashed by the depth
  // ratio, plus a short lip along its front edge so it reads as raised.
  stampGroundPatch() {
    const { mask, w, h } = this;
    const rx = Math.max(1, this.radiusPx);
    const ry = Math.max(1, rx * this.depthRatio);
    const lip = Math.max(0, Math.round(this.heightPx * 0.5));
    const x0 = Math.max(0, Math.floor(this.ox - rx - 1));
    const x1 = Math.min(w - 1, Math.ceil(this.ox + rx + 1));
    const y0 = Math.max(0, Math.floor(this.oy - ry - 1));
    const y1 = Math.min(h - 1, Math.ceil(this.oy + ry + 1));
    const bottom = new Int32Array(w).fill(-1);
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        const dx = (x + 0.5 - this.ox) / rx;
        const dy = (y + 0.5 - this.oy) / ry;
        const d = Math.sqrt(dx * dx + dy * dy);
        const wobble = (hash2(x, y, this.seed) - 0.5) * 0.4;
        if (d > 1 + wobble) continue;
        mask[y * w + x] = MAT.GROUND;
        if (y > bottom[x]) bottom[x] = y;
      }
    }
    for (let x = x0; x <= x1; x++) {
      if (bottom[x] < 0) continue;
      const thick = Math.round(lip * (0.6 + 0.4 * hash2(x, 1, this.seed)));
      for (let k = 1; k <= thick; k++) {
        const y = bottom[x] + k;
        if (y >= h) break;
        mask[y * w + x] = MAT.GROUND;
      }
    }
  }

  markLeafEdges() {
    const { mask, w, h } = this;
    const edges = [];
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const i = y * w + x;
        if (mask[i] !== MAT.LEAF) continue;
        const up = y > 0 ? mask[i - w] : 0;
        const dn = y < h - 1 ? mask[i + w] : 0;
        const lf = x > 0 ? mask[i - 1] : 0;
        const rt = x < w - 1 ? mask[i + 1] : 0;
        if (up !== MAT.LEAF || dn !== MAT.LEAF || lf !== MAT.LEAF || rt !== MAT.LEAF) edges.push(i);
      }
    }
    for (const i of edges) mask[i] = MAT.LEAF_EDGE;
  }

  ensureScratch() {
    const n = this.w * this.h;
    if (!this.scratch) {
      this.scratch = {
        gmask: new Uint8Array(n),
        dist: new Float32Array(n),
        labels: new Int32Array(n),
        stack: new Int32Array(n),
      };
    }
    return this.scratch;
  }

  raster(env) {
    this.mask.fill(MAT.EMPTY);
    this.bias.fill(0);
    this.sprite.fill(0);

    if (this.species.sizeClass === 'ground') {
      this.stampGroundPatch();
    } else {
      for (const seg of this.segments) this.stampSegment(seg);
      for (const leaf of this.leaves) this.stampLeaf(leaf);
      if (this.species.form.leafEdges) this.markLeafEdges();
    }

    this.shade(env);
    this.updateBounds();
    this.dirty = false;
  }

  shade(env) {
    const { mask, bias, sprite, w, h } = this;
    const sc = this.ensureScratch();
    const shading = env.shading;
    const tones = this.species.shade.tones;
    const jitter = this.species.shade.jitter;
    const ramps = env.rampsFor(this.species);

    for (const group of SHADE_GROUPS) {
      let any = false;
      for (let i = 0; i < mask.length; i++) {
        const m = mask[i];
        const inGroup = m !== 0 && group.mats.indexOf(m) !== -1;
        sc.gmask[i] = inGroup ? 1 : 0;
        if (inGroup) any = true;
      }
      if (!any) continue;

      distanceTransform(sc.gmask, w, h, sc.dist);
      const { labels, comps } = labelComponents(sc.gmask, w, h, sc.labels, sc.stack);
      for (let i = 0; i < labels.length; i++) {
        const l = labels[i];
        if (l < 0) continue;
        const d = sc.dist[i];
        if (d > comps[l].maxDepth) comps[l].maxDepth = d;
      }

      const core = Math.max(0.5, this.species.shade[group.core]);
      const adaptive = this.species.shade.adaptiveCore === true;
      for (let y = 0; y < h; y++) {
        for (let x = 0; x < w; x++) {
          const i = y * w + x;
          const l = labels[i];
          if (l < 0) continue;
          const comp = comps[l];
          // Fixed core depth keeps thin twigs light and only lets thick bodies
          // reach the darkest tone; adaptive rescales per shape so every shape
          // uses the full ramp.
          const norm = adaptive ? Math.min(core, Math.max(0.5, comp.maxDepth)) : core;
          const nd = clamp01(sc.dist[i] / norm);
          const span = comp.y1 - comp.y0;
          const vert = span > 0 ? (y - comp.y0) / span : 0;
          let t = shadeValue(nd, vert, shading);
          t += bias[i] / 100 + this.depthShade;
          if (jitter > 0) t += (hash2(x, y, this.seed) - 0.5) * 2 * jitter;
          const q = quantize(clamp01(t), tones);
          const ramp = ramps[mask[i]];
          if (ramp && ramp.length) sprite[i] = rampPick(ramp, q);
        }
      }
    }
  }

  updateBounds() {
    const { mask, w, h } = this;
    let x0 = w;
    let y0 = h;
    let x1 = -1;
    let y1 = -1;
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        if (!mask[y * w + x]) continue;
        if (x < x0) x0 = x;
        if (x > x1) x1 = x;
        if (y < y0) y0 = y;
        if (y > y1) y1 = y;
      }
    }
    this.bounds = { x0, y0, x1, y1 };
  }
}
