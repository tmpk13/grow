// Shading panel: the shared tone curve, plotted and previewed on test shapes.

import { button, clear, el, numberField, section, selectField } from './controls.js';
import { curveValue, defaultShading, quantize, shadeValue } from '../shading.js';
import { findSampler, rampPick, samplerRamp } from '../sampler.js';
import { clamp01, distanceTransform, labelComponents, unpackRGBA } from '../util.js';

export function buildShadingPanel(root, app) {
  const state = app.state;
  clear(root);

  const plot = el('canvas', { class: 'plot-canvas' });
  const plotWrap = el('div', { class: 'plot-wrap' }, [plot]);
  const preview = el('canvas', { class: 'shade-preview' });
  const previewWrap = el('div', { class: 'preview-wrap' }, [preview]);

  const redraw = () => {
    drawCurve(plot, state.shading);
    drawShapes(preview, state, app.ui.shadePreviewSampler, app.ui.shadePreviewTones, app.ui.shadePreviewCore);
  };

  const numeric = (label, key, min, max, step, hint) =>
    numberField(label, {
      value: state.shading[key],
      min,
      max,
      step,
      hint,
      onInput: (v) => {
        state.shading[key] = v;
        redraw();
        app.shadingChanged();
      },
    });

  const fields = [
    numeric('Mid tone', 'mid', 0, 1, 0.01, 'tone before any shading is applied'),
    numeric('Center darker', 'centerDark', 0, 1, 0.01, 'pixels deep inside a shape'),
    numeric('Top edge lighter', 'topLight', 0, 1, 0.01),
    numeric('Bottom edge darker', 'bottomDark', 0, 1, 0.01),
    numeric('Curve start', 'edge0', 0, 1, 0.01, 'below this the response is flat at 0'),
    numeric('Curve end', 'edge1', 0, 1, 0.01, 'above this the response is flat at 1'),
    numeric('Curve gamma', 'gamma', 0.2, 4, 0.05, 'below 1 reaches the plateau sooner'),
  ];

  const presets = el('div', { class: 'btn-row' }, [
    button('Flat body', () => applyPreset(state, { edge0: 0.05, edge1: 0.3, gamma: 1 }, redraw, app)),
    button('Soft', () => applyPreset(state, { edge0: 0.0, edge1: 1.0, gamma: 1 }, redraw, app)),
    button('Rim only', () => applyPreset(state, { edge0: 0.55, edge1: 0.95, gamma: 1.4 }, redraw, app)),
    button('Reset', () => applyPreset(state, defaultShading(), redraw, app)),
  ]);

  const samplerSelect = selectField('Preview material', {
    value: app.ui.shadePreviewSampler,
    options: state.materials.samplers.map((s) => ({ value: s.id, label: s.name })),
    onInput: (v) => {
      app.ui.shadePreviewSampler = v;
      redraw();
    },
  });

  const tonesField = numberField('Preview tone steps', {
    value: app.ui.shadePreviewTones,
    min: 2,
    max: 16,
    step: 1,
    onInput: (v) => {
      app.ui.shadePreviewTones = v | 0;
      redraw();
    },
  });

  const coreField = numberField('Preview core depth (px)', {
    value: app.ui.shadePreviewCore,
    min: 0.5,
    max: 16,
    step: 0.5,
    onInput: (v) => {
      app.ui.shadePreviewCore = v;
      redraw();
    },
    hint: 'depth at which a shape reads as fully core',
  });

  root.appendChild(
    section('Tone curve', [
      plotWrap,
      el('p', { class: 'note', text:
        'x is the input (depth inside the shape, or distance from an edge), y is the curve response. ' +
        'A narrow start-to-end span leaves most of a body on one flat tone.' }),
      ...fields,
      presets,
    ]),
  );
  root.appendChild(section('Preview', [previewWrap, samplerSelect, tonesField, coreField]));

  requestAnimationFrame(redraw);
  return { redraw };
}

function applyPreset(state, patch, redraw, app) {
  Object.assign(state.shading, patch);
  redraw();
  app.shadingChanged();
  app.rebuildPanel();
}

function drawCurve(canvas, shading) {
  const rect = canvas.getBoundingClientRect();
  if (rect.width === 0) return;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(rect.width * dpr);
  canvas.height = Math.round(rect.height * dpr);
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = '#0b0f14';
  ctx.fillRect(0, 0, rect.width, rect.height);

  ctx.strokeStyle = 'rgba(255,255,255,0.08)';
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let i = 1; i < 4; i++) {
    const x = (rect.width * i) / 4;
    const y = (rect.height * i) / 4;
    ctx.moveTo(x, 0);
    ctx.lineTo(x, rect.height);
    ctx.moveTo(0, y);
    ctx.lineTo(rect.width, y);
  }
  ctx.stroke();

  ctx.strokeStyle = '#7fd1a0';
  ctx.lineWidth = 2;
  ctx.beginPath();
  const steps = 128;
  for (let i = 0; i <= steps; i++) {
    const x = i / steps;
    const y = curveValue(x, shading);
    const px = x * rect.width;
    const py = rect.height - y * rect.height;
    if (i === 0) ctx.moveTo(px, py);
    else ctx.lineTo(px, py);
  }
  ctx.stroke();

  // Resulting tone across a slice from edge to core with vert fixed at middle.
  ctx.strokeStyle = 'rgba(255,200,120,0.9)';
  ctx.setLineDash([4, 3]);
  ctx.beginPath();
  for (let i = 0; i <= steps; i++) {
    const x = i / steps;
    const t = shadeValue(x, 0.5, shading);
    const px = x * rect.width;
    const py = rect.height - t * rect.height;
    if (i === 0) ctx.moveTo(px, py);
    else ctx.lineTo(px, py);
  }
  ctx.stroke();
  ctx.setLineDash([]);
}

// Test shapes: a thick trunk slab, a round blob and a leaf ellipse, shaded by
// exactly the same rules the sim uses.
function buildTestMask(w, h) {
  const mask = new Uint8Array(w * h);
  const trunkX0 = Math.round(w * 0.06);
  const trunkX1 = Math.round(w * 0.2);
  for (let y = Math.round(h * 0.1); y < h - 2; y++) {
    for (let x = trunkX0; x <= trunkX1; x++) mask[y * w + x] = 1;
  }
  const bx = w * 0.48;
  const by = h * 0.45;
  const br = Math.min(w, h) * 0.28;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const dx = x + 0.5 - bx;
      const dy = y + 0.5 - by;
      if (dx * dx + dy * dy <= br * br) mask[y * w + x] = 1;
    }
  }
  const lx = w * 0.82;
  const ly = h * 0.5;
  const rx = w * 0.13;
  const ry = h * 0.3;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const dx = (x + 0.5 - lx) / rx;
      const dy = (y + 0.5 - ly) / ry;
      if (dx * dx + dy * dy <= 1) mask[y * w + x] = 1;
    }
  }
  return mask;
}

function drawShapes(canvas, state, samplerId, tones, core) {
  const rect = canvas.getBoundingClientRect();
  if (rect.width === 0) return;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(rect.width * dpr);
  canvas.height = Math.round(rect.height * dpr);
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = '#0b0f14';
  ctx.fillRect(0, 0, rect.width, rect.height);

  const w = 72;
  const h = 40;
  const mask = buildTestMask(w, h);
  const dist = distanceTransform(mask, w, h);
  const { labels, comps } = labelComponents(mask, w, h);
  for (let i = 0; i < labels.length; i++) {
    const l = labels[i];
    if (l < 0) continue;
    if (dist[i] > comps[l].maxDepth) comps[l].maxDepth = dist[i];
  }

  const sampler = findSampler(state.materials, samplerId) || state.materials.samplers[0];
  const ramp = samplerRamp(state.materials, sampler);
  const off = document.createElement('canvas');
  off.width = w;
  off.height = h;
  const octx = off.getContext('2d');
  const img = octx.createImageData(w, h);
  const data = img.data;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      const l = labels[i];
      if (l < 0) continue;
      const comp = comps[l];
      const norm = Math.min(core, Math.max(0.5, comp.maxDepth));
      const nd = clamp01(dist[i] / norm);
      const span = comp.y1 - comp.y0;
      const vert = span > 0 ? (y - comp.y0) / span : 0;
      const t = quantize(shadeValue(nd, vert, state.shading), tones);
      const c = unpackRGBA(rampPick(ramp, t));
      const o = i * 4;
      data[o] = c.r;
      data[o + 1] = c.g;
      data[o + 2] = c.b;
      data[o + 3] = 255;
    }
  }
  octx.putImageData(img, 0, 0);
  const z = Math.max(1, Math.min(rect.width / w, rect.height / h));
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(off, (rect.width - w * z) / 2, (rect.height - h * z) / 2, w * z, h * z);
}
