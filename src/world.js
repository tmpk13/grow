// The world grid.
//
// Side view: columns run left to right, rows top to bottom. Rows at or below
// soilRow are soil, everything above is air. A plant is rooted in one column
// on the soil surface and claims the cells its body covers.
//
// Occupancy is tracked per size class layer, which is what allows several
// items to share a cell (ground cover plus a tree) while still forbidding two
// items of the same class in one place.

import { SIZE_CLASSES } from './species.js';

export const LAYER_COUNT = Object.keys(SIZE_CLASSES).length;

export function defaultWorldConfig() {
  return {
    cols: 72,
    rows: 44,
    cellPx: 8,
    soilRow: 38,
    skyTop: '#101a26',
    skyBottom: '#2c3a3f',
    soilSampler: 'mat-soil',
  };
}

export class World {
  constructor(cfg) {
    this.configure(cfg);
  }

  configure(cfg) {
    this.cols = Math.max(4, cfg.cols | 0);
    this.rows = Math.max(4, cfg.rows | 0);
    this.cellPx = Math.max(1, cfg.cellPx | 0);
    this.soilRow = Math.min(this.rows - 1, Math.max(1, cfg.soilRow | 0));
    this.pxW = this.cols * this.cellPx;
    this.pxH = this.rows * this.cellPx;
    this.surfacePx = this.soilRow * this.cellPx;
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

  occupant(layer, cx, cy) {
    if (!this.inBounds(cx, cy)) return 0;
    return this.layers[layer][this.idx(cx, cy)];
  }

  // Cells a plant of the given footprint would cover. Returned as flat indices.
  footprint(cx, radiusCells, heightPx, out = []) {
    out.length = 0;
    const rowsUp = Math.max(1, Math.ceil(heightPx / this.cellPx));
    const x0 = Math.max(0, cx - radiusCells);
    const x1 = Math.min(this.cols - 1, cx + radiusCells);
    const y1 = this.soilRow;
    const y0 = Math.max(0, this.soilRow - rowsUp);
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) out.push(y * this.cols + x);
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

  // Spacing test along the surface row for the same layer.
  hasNeighborWithin(layer, cx, spacing) {
    if (spacing <= 0) return false;
    const grid = this.layers[layer];
    const y = this.soilRow;
    for (let dx = -spacing; dx <= spacing; dx++) {
      const x = cx + dx;
      if (x < 0 || x >= this.cols) continue;
      if (grid[y * this.cols + x] !== 0) return true;
    }
    return false;
  }

  // Nearest woody support for a climbing plant: scans outward along the
  // surface row for a tree or shrub and returns its column and owner id.
  findSupport(cx, searchCells, supportLayers) {
    for (let d = 0; d <= searchCells; d++) {
      for (const dir of d === 0 ? [0] : [-1, 1]) {
        const x = cx + dir * d;
        if (x < 0 || x >= this.cols) continue;
        for (const layer of supportLayers) {
          const owner = this.layers[layer][this.soilRow * this.cols + x];
          if (owner !== 0) return { col: x, owner, layer };
        }
      }
    }
    return null;
  }

  // Occupancy heat per cell, used by the debug overlay.
  occupancyAt(cx, cy) {
    let mask = 0;
    for (let l = 0; l < LAYER_COUNT; l++) {
      if (this.layers[l][this.idx(cx, cy)] !== 0) mask |= 1 << l;
    }
    return mask;
  }
}
