// The world grid, a 2.5D area.
//
// The grid is a ground plane seen at an angle: columns run left to right (x)
// and rows run from the back of the area to the front (depth). A cell is drawn
// cellPx wide and depthPx tall, so a row of depth is foreshortened, and plants
// stand up out of their cell toward the top of the screen.
//
//   screen x = gx * cellPx
//   screen y = skyPx + gy * depthPx      (row 0 is the far edge)
//
// Occupancy is tracked per size class layer, which is what allows several
// items to occupy one cell (ground cover plus a tree) while still forbidding
// two items of the same class in one place.

import { SIZE_CLASSES } from './species.js';

export const LAYER_COUNT = Object.keys(SIZE_CLASSES).length;

export function defaultWorldConfig() {
  return {
    cols: 64,
    rows: 24,
    cellPx: 8,
    depthPx: 5,
    skyPx: 150,
    skyTop: '#101a26',
    skyBottom: '#33424a',
    soilSampler: 'mat-soil',
    depthFade: 0.16,
    shadows: true,
  };
}

export class World {
  constructor(cfg) {
    this.configure(cfg);
  }

  configure(cfg) {
    this.cols = Math.max(4, cfg.cols | 0);
    this.rows = Math.max(2, cfg.rows | 0);
    this.cellPx = Math.max(1, cfg.cellPx | 0);
    this.depthPx = Math.max(1, cfg.depthPx | 0);
    this.skyPx = Math.max(0, cfg.skyPx | 0);
    this.depthRatio = this.depthPx / this.cellPx;
    this.pxW = this.cols * this.cellPx;
    this.groundPx = this.rows * this.depthPx;
    // Ground past the front row, wide enough that a mat centered on the
    // nearest row is not clipped by the bottom of the buffer.
    this.frontPx = Math.max(this.cellPx, this.depthPx * 3);
    this.pxH = this.skyPx + this.groundPx + this.frontPx;
    this.layers = [];
    for (let i = 0; i < LAYER_COUNT; i++) this.layers.push(new Int32Array(this.cols * this.rows));
  }

  clear() {
    for (const layer of this.layers) layer.fill(0);
  }

  inBounds(cx, cy) {
    return cx >= 0 && cx < this.cols && cy >= 0 && cy < this.rows;
  }

  idx(cx, cy) {
    return cy * this.cols + cx;
  }

  // Screen position a plant rooted in this cell is anchored to: the middle of
  // the cell on the ground plane.
  anchorX(cx) {
    return cx * this.cellPx + Math.floor(this.cellPx / 2);
  }

  anchorY(cy) {
    return this.skyPx + cy * this.depthPx + Math.floor(this.depthPx / 2);
  }

  occupant(layer, cx, cy) {
    if (!this.inBounds(cx, cy)) return 0;
    return this.layers[layer][this.idx(cx, cy)];
  }

  // Cells covered by a footprint of the given radius, as a disc on the ground
  // plane. Returned as flat indices.
  footprint(cx, cy, radiusCells, out = []) {
    out.length = 0;
    const r = Math.max(0, radiusCells);
    const r2 = (r + 0.35) * (r + 0.35);
    for (let y = cy - r; y <= cy + r; y++) {
      if (y < 0 || y >= this.rows) continue;
      for (let x = cx - r; x <= cx + r; x++) {
        if (x < 0 || x >= this.cols) continue;
        const dx = x - cx;
        const dy = y - cy;
        if (dx * dx + dy * dy > r2) continue;
        out.push(y * this.cols + x);
      }
    }
    return out;
  }

  // True when every cell is free or already owned by this instance.
  canClaim(layer, cells, instanceId) {
    const grid = this.layers[layer];
    for (let i = 0; i < cells.length; i++) {
      const owner = grid[cells[i]];
      if (owner !== 0 && owner !== instanceId) return false;
    }
    return true;
  }

  claim(layer, cells, instanceId) {
    const grid = this.layers[layer];
    for (let i = 0; i < cells.length; i++) grid[cells[i]] = instanceId;
  }

  release(layer, cells, instanceId) {
    const grid = this.layers[layer];
    for (let i = 0; i < cells.length; i++) {
      if (grid[cells[i]] === instanceId) grid[cells[i]] = 0;
    }
  }

  // Spacing test over the area, same layer.
  hasNeighborWithin(layer, cx, cy, spacing) {
    if (spacing <= 0) return false;
    const grid = this.layers[layer];
    const r2 = spacing * spacing;
    for (let y = cy - spacing; y <= cy + spacing; y++) {
      if (y < 0 || y >= this.rows) continue;
      for (let x = cx - spacing; x <= cx + spacing; x++) {
        if (x < 0 || x >= this.cols) continue;
        const dx = x - cx;
        const dy = y - cy;
        if (dx * dx + dy * dy > r2) continue;
        if (grid[y * this.cols + x] !== 0) return true;
      }
    }
    return false;
  }

  // Nearest woody support for a climbing plant: searches the area outward and
  // returns the closest occupied cell in one of the given layers.
  findSupport(cx, cy, searchCells, supportLayers) {
    let best = null;
    let bestDist = Infinity;
    for (let y = cy - searchCells; y <= cy + searchCells; y++) {
      if (y < 0 || y >= this.rows) continue;
      for (let x = cx - searchCells; x <= cx + searchCells; x++) {
        if (x < 0 || x >= this.cols) continue;
        const dx = x - cx;
        const dy = y - cy;
        const d = dx * dx + dy * dy;
        if (d > searchCells * searchCells || d >= bestDist) continue;
        for (const layer of supportLayers) {
          const owner = this.layers[layer][y * this.cols + x];
          if (owner !== 0) {
            best = { col: x, row: y, owner, layer };
            bestDist = d;
            break;
          }
        }
      }
    }
    return best;
  }

  // Occupancy bitmask per cell, used by the debug overlay.
  occupancyAt(cx, cy) {
    let mask = 0;
    for (let l = 0; l < LAYER_COUNT; l++) {
      if (this.layers[l][this.idx(cx, cy)] !== 0) mask |= 1 << l;
    }
    return mask;
  }
}
