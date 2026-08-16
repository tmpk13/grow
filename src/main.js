// Application shell: tabs, the growth test window and the frame loop.

import { createState, deserializeState, loadLocal, saveLocal, serializeState } from './state.js';
import { Sim } from './sim.js';
import { Viewport } from './render.js';
import { invalidateSamplerCache } from './sampler.js';
import { button, clear, el } from './ui/controls.js';
import { buildMaterialsPanel } from './ui/materialsPanel.js';
import { buildShadingPanel } from './ui/shadingPanel.js';
import { buildSpeciesPanel } from './ui/speciesPanel.js';
import { buildWorldPanel } from './ui/worldPanel.js';
import { clamp } from './util.js';

const TABS = [
  { id: 'materials', label: 'Materials', build: buildMaterialsPanel },
  { id: 'shading', label: 'Shading', build: buildShadingPanel },
  { id: 'species', label: 'Species', build: buildSpeciesPanel },
  { id: 'world', label: 'World', build: buildWorldPanel },
];

const state = loadLocal() || createState();
const sim = new Sim(state);
const canvas = document.getElementById('world-canvas');
const viewport = new Viewport(canvas);

let currentPanel = null;
let saveTimer = 0;

const app = {
  state,
  sim,
  viewport,
  env: sim.env,
  ui: {
    tab: 'materials',
    selectedSamplerId: state.materials.samplers[0]?.id,
    selectedSpeciesId: state.species[0]?.id,
    brushColor: null,
    tool: 'pencil',
    mirrorX: false,
    shadePreviewSampler: state.materials.samplers[0]?.id,
    shadePreviewTones: 5,
    shadePreviewCore: 4,
  },
  materialsChanged() {
    state.materials.version++;
    invalidateSamplerCache();
    sim.env.invalidate();
    sim.markAllDirty();
    app.requestSave();
  },
  shadingChanged() {
    sim.markAllDirty();
    app.requestSave();
  },
  speciesChanged() {
    sim.env.invalidate();
    sim.markAllDirty();
    if (currentPanel && currentPanel.redraw) currentPanel.redraw();
    app.requestSave();
  },
  worldChanged() {
    sim.resizeWorld();
    viewport.fit(sim.world);
    app.requestSave();
  },
  repaintBackground() {
    sim.bufferDirty = true;
    app.requestSave();
  },
  restart() {
    sim.reset(state.seed);
    viewport.fit(sim.world);
  },
  requestSave() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      const ok = saveLocal(state);
      setSaveNote(ok ? `saved ${new Date().toLocaleTimeString()}` : 'save failed');
    }, 600);
  },
  rebuildPanel() {
    showTab(app.ui.tab);
  },
};

// ---- tabs ----------------------------------------------------------------

const tabsNode = document.getElementById('tabs');
const panelBody = document.getElementById('panel-body');

function showTab(id) {
  app.ui.tab = id;
  clear(tabsNode);
  for (const tab of TABS) {
    tabsNode.appendChild(
      el('button', {
        class: `tab${tab.id === id ? ' active' : ''}`,
        type: 'button',
        text: tab.label,
        onclick: () => showTab(tab.id),
      }),
    );
  }
  const def = TABS.find((t) => t.id === id) || TABS[0];
  currentPanel = def.build(panelBody, app);
}

// ---- stage toolbar -------------------------------------------------------

const toolbar = document.getElementById('stage-toolbar');
const status = document.getElementById('statusbar');
const saveNote = document.getElementById('save-note');

function setSaveNote(text) {
  if (saveNote) saveNote.textContent = text;
}

const playBtn = button('Play', () => {
  state.sim.running = !state.sim.running;
  playBtn.textContent = state.sim.running ? 'Pause' : 'Play';
  app.requestSave();
});
playBtn.textContent = state.sim.running ? 'Pause' : 'Play';

const speedInput = el('input', { type: 'range', min: 0.25, max: 32, step: 0.25, value: state.sim.speed });
const speedLabel = el('span', { class: 'readout', text: `${state.sim.speed}x` });
speedInput.addEventListener('input', () => {
  state.sim.speed = Number(speedInput.value);
  speedLabel.textContent = `${state.sim.speed}x`;
  app.requestSave();
});

const zoomInput = el('input', { type: 'range', min: 0.5, max: 16, step: 0.25, value: viewport.zoom });
const zoomLabel = el('span', { class: 'readout', text: `${viewport.zoom.toFixed(2)}x` });
zoomInput.addEventListener('input', () => {
  const rect = canvas.getBoundingClientRect();
  const target = Number(zoomInput.value);
  viewport.zoomAt(rect.left + rect.width / 2, rect.top + rect.height / 2, target / viewport.zoom);
  zoomLabel.textContent = `${viewport.zoom.toFixed(2)}x`;
});

const gridToggle = el('input', { type: 'checkbox' });
gridToggle.addEventListener('change', () => {
  viewport.showGrid = gridToggle.checked;
});
const occToggle = el('input', { type: 'checkbox' });
occToggle.addEventListener('change', () => {
  viewport.showOccupancy = occToggle.checked;
});

toolbar.appendChild(
  el('div', { class: 'toolbar-row' }, [
    playBtn,
    button('Step', () => sim.step(1 / state.sim.tickHz)),
    button('Restart', () => app.restart()),
    el('label', { class: 'inline' }, [el('span', { text: 'Speed' }), speedInput, speedLabel]),
    el('label', { class: 'inline' }, [el('span', { text: 'Zoom' }), zoomInput, zoomLabel]),
    button('Fit', () => {
      viewport.fit(sim.world);
      zoomInput.value = viewport.zoom;
      zoomLabel.textContent = `${viewport.zoom.toFixed(2)}x`;
    }),
    el('label', { class: 'inline' }, [el('span', { text: 'Grid' }), gridToggle]),
    el('label', { class: 'inline' }, [el('span', { text: 'Occupancy' }), occToggle]),
  ]),
);

// ---- canvas interaction --------------------------------------------------

canvas.addEventListener('wheel', (e) => {
  e.preventDefault();
  viewport.zoomAt(e.clientX, e.clientY, e.deltaY < 0 ? 1.12 : 1 / 1.12);
  zoomInput.value = clamp(viewport.zoom, 0.5, 16);
  zoomLabel.textContent = `${viewport.zoom.toFixed(2)}x`;
}, { passive: false });

let dragging = false;
let lastX = 0;
let lastY = 0;
canvas.addEventListener('pointerdown', (e) => {
  dragging = true;
  lastX = e.clientX;
  lastY = e.clientY;
  canvas.setPointerCapture(e.pointerId);
});
canvas.addEventListener('pointermove', (e) => {
  if (!dragging) return;
  viewport.pan(e.clientX - lastX, e.clientY - lastY);
  lastX = e.clientX;
  lastY = e.clientY;
});
const endDrag = () => {
  dragging = false;
};
canvas.addEventListener('pointerup', endDrag);
canvas.addEventListener('pointercancel', endDrag);

window.addEventListener('keydown', (e) => {
  if (e.target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(e.target.tagName)) return;
  if (e.code === 'Space') {
    e.preventDefault();
    playBtn.click();
  } else if (e.key === '.') {
    sim.step(1 / state.sim.tickHz);
  } else if (e.key === 'f') {
    viewport.fit(sim.world);
  }
});

// ---- project actions -----------------------------------------------------

document.getElementById('btn-new').addEventListener('click', () => {
  const fresh = createState();
  Object.assign(state, fresh);
  invalidateSamplerCache();
  sim.env.invalidate();
  sim.reset(state.seed);
  viewport.fit(sim.world);
  app.ui.selectedSamplerId = state.materials.samplers[0].id;
  app.ui.selectedSpeciesId = state.species[0].id;
  app.rebuildPanel();
  app.requestSave();
});

document.getElementById('btn-export').addEventListener('click', () => {
  const blob = new Blob([JSON.stringify(serializeState(state), null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = el('a', { href: url, download: 'grow-project.json' });
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
});

const importInput = document.getElementById('file-import');
importInput.addEventListener('change', async () => {
  const file = importInput.files && importInput.files[0];
  if (!file) return;
  try {
    const data = JSON.parse(await file.text());
    const next = deserializeState(data);
    Object.assign(state, next);
    sim.env.invalidate();
    sim.reset(state.seed);
    viewport.fit(sim.world);
    app.ui.selectedSamplerId = state.materials.samplers[0].id;
    app.ui.selectedSpeciesId = state.species[0].id;
    app.rebuildPanel();
    setSaveNote(`imported ${file.name}`);
    app.requestSave();
  } catch (err) {
    setSaveNote(`import failed: ${err.message}`);
  }
  importInput.value = '';
});

// ---- frame loop ----------------------------------------------------------

let last = performance.now();
let accumulator = 0;
let fps = 0;

function frame(ts) {
  const dtReal = Math.min(0.1, (ts - last) / 1000);
  last = ts;
  fps = fps * 0.9 + (1 / Math.max(1e-3, dtReal)) * 0.1;

  if (state.sim.running) {
    const stepDt = 1 / state.sim.tickHz;
    accumulator += dtReal * state.sim.speed;
    let steps = 0;
    while (accumulator >= stepDt && steps < 400) {
      sim.step(stepDt);
      accumulator -= stepDt;
      steps++;
    }
    if (accumulator > 2) accumulator = 0;
  }

  sim.processRasterQueue(state.sim.rasterBudget);
  viewport.draw(sim);
  if (currentPanel && currentPanel.tick) currentPanel.tick(dtReal);
  updateStatus();
  requestAnimationFrame(frame);
}

function updateStatus() {
  const s = sim.stats();
  const parts = [
    `tick ${s.ticks}`,
    `sim time ${s.time.toFixed(1)}`,
    `plants ${s.total}`,
    `queue ${sim.rasterQueue.length}`,
    `${fps.toFixed(0)} fps`,
  ];
  for (const sp of state.species) {
    parts.push(`${sp.name}: ${s.perSpecies.get(sp.id) || 0}`);
  }
  status.textContent = parts.join('   ');
}

// ---- boot ----------------------------------------------------------------

const resizeObserver = new ResizeObserver(() => {
  viewport.resize();
});
resizeObserver.observe(canvas.parentElement);

showTab('materials');
viewport.fit(sim.world);
requestAnimationFrame(frame);
