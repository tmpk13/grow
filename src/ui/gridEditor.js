// Drawable pixel grid used for every sampling box (and for the single shared
// atlas). Works on whatever buffer getGrid() hands back.

import { EMPTY_COLOR, packedToHex } from '../util.js';

export class GridEditor {
  constructor(canvas, { getGrid, onCommit, getOverlays }) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.getGrid = getGrid;
    this.onCommit = onCommit || (() => {});
    this.getOverlays = getOverlays || (() => []);
    this.tool = 'pencil';
    this.color = 0;
    this.mirrorX = false;
    this.onPick = null;
    this.drawing = false;
    this.last = null;
    this.bindEvents();
  }

  bindEvents() {
    const c = this.canvas;
    c.addEventListener('pointerdown', (e) => {
      c.setPointerCapture(e.pointerId);
      this.drawing = true;
      this.last = this.cellAt(e);
      this.apply(this.last, e);
    });
    c.addEventListener('pointermove', (e) => {
      if (!this.drawing) return;
      const cell = this.cellAt(e);
      if (!cell || (this.last && cell.x === this.last.x && cell.y === this.last.y)) return;
      if (this.last && (this.tool === 'pencil' || this.tool === 'eraser')) {
        this.strokeLine(this.last, cell);
      } else {
        this.apply(cell, e);
      }
      this.last = cell;
    });
    const stop = () => {
      if (!this.drawing) return;
      this.drawing = false;
      this.last = null;
      this.onCommit();
    };
    c.addEventListener('pointerup', stop);
    c.addEventListener('pointercancel', stop);
    c.addEventListener('pointerleave', stop);
    c.addEventListener('contextmenu', (e) => e.preventDefault());
  }

  cellAt(evt) {
    const g = this.getGrid();
    if (!g) return null;
    const rect = this.canvas.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return null;
    const x = Math.floor(((evt.clientX - rect.left) / rect.width) * g.w);
    const y = Math.floor(((evt.clientY - rect.top) / rect.height) * g.h);
    if (x < 0 || y < 0 || x >= g.w || y >= g.h) return null;
    return { x, y };
  }

  strokeLine(a, b) {
    const steps = Math.max(Math.abs(b.x - a.x), Math.abs(b.y - a.y));
    for (let i = 1; i <= steps; i++) {
      const t = i / steps;
      this.apply({ x: Math.round(a.x + (b.x - a.x) * t), y: Math.round(a.y + (b.y - a.y) * t) });
    }
  }

  apply(cell, evt) {
    if (!cell) return;
    const g = this.getGrid();
    if (!g) return;
    const erase = this.tool === 'eraser' || (evt && (evt.buttons & 2) === 2);
    if (this.tool === 'pick') {
      const v = g.px[cell.y * g.w + cell.x];
      if (this.onPick) this.onPick(v);
      return;
    }
    if (this.tool === 'fill') {
      this.floodFill(g, cell.x, cell.y, erase ? EMPTY_COLOR : this.color);
      this.draw();
      return;
    }
    const value = erase ? EMPTY_COLOR : this.color;
    g.px[cell.y * g.w + cell.x] = value;
    if (this.mirrorX) g.px[cell.y * g.w + (g.w - 1 - cell.x)] = value;
    this.draw();
  }

  floodFill(g, x, y, value) {
    const target = g.px[y * g.w + x];
    if (target === value) return;
    const stack = [y * g.w + x];
    while (stack.length) {
      const i = stack.pop();
      if (g.px[i] !== target) continue;
      g.px[i] = value;
      const cx = i % g.w;
      const cy = (i / g.w) | 0;
      if (cx > 0) stack.push(i - 1);
      if (cx < g.w - 1) stack.push(i + 1);
      if (cy > 0) stack.push(i - g.w);
      if (cy < g.h - 1) stack.push(i + g.w);
    }
  }

  draw() {
    const g = this.getGrid();
    const canvas = this.canvas;
    const rect = canvas.getBoundingClientRect();
    if (!g || rect.width === 0) return;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(rect.width * dpr));
    const h = Math.max(1, Math.round(rect.height * dpr));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    const ctx = this.ctx;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, rect.width, rect.height);
    const cw = rect.width / g.w;
    const ch = rect.height / g.h;

    for (let y = 0; y < g.h; y++) {
      for (let x = 0; x < g.w; x++) {
        const v = g.px[y * g.w + x];
        if (v === EMPTY_COLOR) {
          ctx.fillStyle = (x + y) % 2 === 0 ? '#1a1f26' : '#141920';
        } else {
          ctx.fillStyle = packedToHex(v);
        }
        ctx.fillRect(x * cw, y * ch, Math.ceil(cw), Math.ceil(ch));
      }
    }

    if (Math.min(cw, ch) >= 7) {
      ctx.strokeStyle = 'rgba(255,255,255,0.07)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let x = 1; x < g.w; x++) {
        ctx.moveTo(Math.round(x * cw) + 0.5, 0);
        ctx.lineTo(Math.round(x * cw) + 0.5, rect.height);
      }
      for (let y = 1; y < g.h; y++) {
        ctx.moveTo(0, Math.round(y * ch) + 0.5);
        ctx.lineTo(rect.width, Math.round(y * ch) + 0.5);
      }
      ctx.stroke();
    }

    for (const ov of this.getOverlays()) {
      ctx.strokeStyle = ov.color;
      ctx.lineWidth = ov.active ? 2 : 1;
      ctx.strokeRect(ov.x * cw + 1, ov.y * ch + 1, ov.w * cw - 2, ov.h * ch - 2);
      if (ov.label) {
        ctx.fillStyle = ov.color;
        ctx.font = '0.7rem system-ui, sans-serif';
        ctx.fillText(ov.label, ov.x * cw + 4, ov.y * ch + 12);
      }
    }
  }
}
