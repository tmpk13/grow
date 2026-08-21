// Canvas presentation: blits the world pixel buffer through a zoomable,
// pannable camera and draws the debug overlays on top.

import { clamp } from './util.js';
import { LAYER_COUNT } from './world.js';

const LAYER_COLORS = [
  'rgba(120, 220, 140, 0.30)',
  'rgba(230, 220, 110, 0.30)',
  'rgba(120, 190, 240, 0.30)',
  'rgba(240, 140, 120, 0.30)',
  'rgba(200, 130, 240, 0.30)',
];

export class Viewport {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.zoom = 2;
    this.panX = 0;
    this.panY = 0;
    this.dpr = 1;
    this.off = document.createElement('canvas');
    this.offCtx = this.off.getContext('2d');
    this.image = null;
    this.showGrid = false;
    this.showOccupancy = false;
  }

  resize() {
    const rect = this.canvas.getBoundingClientRect();
    this.dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(rect.width * this.dpr));
    const h = Math.max(1, Math.round(rect.height * this.dpr));
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
  }

  ensureImage(world) {
    if (this.off.width !== world.pxW || this.off.height !== world.pxH) {
      this.off.width = world.pxW;
      this.off.height = world.pxH;
      this.image = this.offCtx.createImageData(world.pxW, world.pxH);
      this.imageU32 = new Uint32Array(this.image.data.buffer);
    }
  }

  fit(world) {
    const rect = this.canvas.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    const zx = rect.width / world.pxW;
    const zy = rect.height / world.pxH;
    this.zoom = clamp(Math.min(zx, zy), 0.25, 24);
    this.panX = (rect.width - world.pxW * this.zoom) / 2;
    this.panY = (rect.height - world.pxH * this.zoom) / 2;
  }

  zoomAt(clientX, clientY, factor) {
    const rect = this.canvas.getBoundingClientRect();
    const cx = clientX - rect.left;
    const cy = clientY - rect.top;
    const next = clamp(this.zoom * factor, 0.25, 32);
    const k = next / this.zoom;
    this.panX = cx - (cx - this.panX) * k;
    this.panY = cy - (cy - this.panY) * k;
    this.zoom = next;
  }

  pan(dx, dy) {
    this.panX += dx;
    this.panY += dy;
  }

  draw(sim) {
    const world = sim.world;
    this.resize();
    this.ensureImage(world);
    if (sim.bufferDirty) sim.composite();
    this.imageU32.set(sim.buffer);
    this.offCtx.putImageData(this.image, 0, 0);

    const ctx = this.ctx;
    ctx.save();
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    const rect = this.canvas.getBoundingClientRect();
    ctx.clearRect(0, 0, rect.width, rect.height);
    ctx.fillStyle = '#05070a';
    ctx.fillRect(0, 0, rect.width, rect.height);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(this.off, this.panX, this.panY, world.pxW * this.zoom, world.pxH * this.zoom);

    if (sim.overlay) sim.overlay(ctx, this);
    if (this.showOccupancy) this.drawOccupancy(ctx, world);
    if (this.showGrid) this.drawGrid(ctx, world);
    ctx.restore();
  }

  // The ground plane is axis aligned but foreshortened, so cells are drawn as
  // rectangles cellPx wide by depthPx tall, offset below the sky band.
  drawGrid(ctx, world) {
    const stepX = world.cellPx * this.zoom;
    const stepY = world.depthPx * this.zoom;
    if (Math.min(stepX, stepY) < 2) return;
    const top = this.panY + world.skyPx * this.zoom;
    ctx.strokeStyle = 'rgba(255,255,255,0.10)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let x = 0; x <= world.cols; x++) {
      const px = Math.round(this.panX + x * stepX) + 0.5;
      ctx.moveTo(px, top);
      ctx.lineTo(px, top + world.groundPx * this.zoom);
    }
    for (let y = 0; y <= world.rows; y++) {
      const py = Math.round(top + y * stepY) + 0.5;
      ctx.moveTo(this.panX, py);
      ctx.lineTo(this.panX + world.pxW * this.zoom, py);
    }
    ctx.stroke();
    ctx.strokeStyle = 'rgba(255,180,90,0.55)';
    ctx.beginPath();
    ctx.moveTo(this.panX, Math.round(top) + 0.5);
    ctx.lineTo(this.panX + world.pxW * this.zoom, Math.round(top) + 0.5);
    ctx.stroke();
  }

  drawOccupancy(ctx, world) {
    const stepX = world.cellPx * this.zoom;
    const stepY = world.depthPx * this.zoom;
    const top = this.panY + world.skyPx * this.zoom;
    for (let cy = 0; cy < world.rows; cy++) {
      for (let cx = 0; cx < world.cols; cx++) {
        const mask = world.occupancyAt(cx, cy);
        if (!mask) continue;
        for (let l = 0; l < LAYER_COUNT; l++) {
          if (!(mask & (1 << l))) continue;
          ctx.fillStyle = LAYER_COLORS[l % LAYER_COLORS.length];
          const insetX = (stepX / (LAYER_COUNT + 1)) * l;
          const insetY = (stepY / (LAYER_COUNT + 1)) * l;
          ctx.fillRect(
            this.panX + cx * stepX + insetX * 0.5,
            top + cy * stepY + insetY * 0.5,
            Math.max(1, stepX - insetX),
            Math.max(1, stepY - insetY),
          );
        }
      }
    }
  }
}

// Draws a single plant sprite centered in a canvas: used by the species
// preview and by any place that needs to show one specimen.
export function drawPlantPreview(canvas, plant, zoom) {
  const ctx = canvas.getContext('2d');
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(rect.width * dpr));
  const h = Math.max(1, Math.round(rect.height * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, rect.width, rect.height);
  ctx.fillStyle = '#0b0f14';
  ctx.fillRect(0, 0, rect.width, rect.height);
  if (!plant || plant.bounds.x1 < plant.bounds.x0) return;

  const off = document.createElement('canvas');
  off.width = plant.w;
  off.height = plant.h;
  const octx = off.getContext('2d');
  const img = octx.createImageData(plant.w, plant.h);
  new Uint32Array(img.data.buffer).set(plant.sprite);
  octx.putImageData(img, 0, 0);

  // Framed on the whole sprite rather than the current silhouette, so the view
  // does not rescale on every growth step.
  const z = zoom || clamp(Math.floor(Math.min(rect.width / plant.w, rect.height / plant.h)), 1, 10);
  const dx = Math.round((rect.width - plant.w * z) / 2);
  const dy = Math.round((rect.height - plant.h * z) / 2);
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(off, 0, 0, plant.w, plant.h, dx, dy, plant.w * z, plant.h * z);
}
