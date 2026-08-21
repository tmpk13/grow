// Application shell: the two modes, their tabs, the stage toolbars and the
// frame loop.
//
// The plant lab and the settlement are two views onto the same project: the
// species and sampling boxes authored in the lab are what grows on the
// settlement map and what its buildings are drawn from.

import { createState, deserializeState, loadLocal, saveLocal, serializeState } from './state.js';
import { Sim } from './sim.js';
import { Settlement } from './civ/settlement.js';
import { Viewport } from './render.js';
import { invalidateSamplerCache } from './sampler.js';
import { button, clear, el } from './ui/controls.js';
import { buildMaterialsPanel } from './ui/materialsPanel.js';
import { buildShadingPanel } from './ui/shadingPanel.js';
import { buildSpeciesPanel } from './ui/speciesPanel.js';
import { buildWorldPanel } from './ui/worldPanel.js';
import { buildLandPanel } from './ui/landPanel.js';
import { buildPeoplePanel } from './ui/peoplePanel.js';
import { buildBuildPanel } from './ui/buildPanel.js';
import { buildEconomyPanel } from './ui/economyPanel.js';
import { buildTechPanel } from './ui/techPanel.js';
import { PROFESSIONS } from './civ/people.js';
import { clamp } from './util.js';

const MODES = [
  {
    id: 'lab',
    label: 'Plant lab',
    tabs: [
      { id: 'materials', label: 'Materials', build: buildMaterialsPanel },
      { id: 'shading', label: 'Shading', build: buildShadingPanel },
      { id: 'species', label: 'Species', build: buildSpeciesPanel },
      { id: 'world', label: 'World', build: buildWorldPanel },
    ],
  },
  {
    id: 'settlement',
    label: 'Settlement',
    tabs: [
      { id: 'land', label: 'Land', build: buildLandPanel },
      { id: 'people', label: 'People', build: buildPeoplePanel },
      { id: 'build', label: 'Build', build: buildBuildPanel },
      { id: 'economy', label: 'Economy', build: buildEconomyPanel },
      { id: 'tech', label: 'Tech', build: buildTechPanel },
    ],
  },
];

const state = loadLocal() || createState();
const sim = new Sim(state);
const canvas = document.getElementById('world-canvas');
const viewport = new Viewport(canvas);

let currentPanel = null;
let settlement = null;
let saveTimer = 0;
let mode = 'lab';

const app = {
  state,
  sim,
  viewport,
  env: sim.env,
  get civ() {
    return settlement;
  },
  get mode() {
    return mode;
  },
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
    if (settlement) {
      settlement.invalidateSprites();
      settlement.markAllDirty();
    }
    app.requestSave();
  },
  shadingChanged() {
    sim.markAllDirty();
    if (settlement) settlement.markAllDirty();
    app.requestSave();
  },
  speciesChanged() {
    sim.env.invalidate();
    sim.markAllDirty();
    if (settlement) settlement.markAllDirty();
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
  // ---- settlement ----
  civRepaint() {
    if (!settlement) return;
    settlement.invalidateSprites();
    app.requestSave();
  },
  civRestart() {
    if (!settlement) return;
    app.setNote('growing the wilderness...');
    // Yielding first lets the note paint before the warmup blocks the thread.
    setTimeout(() => {
      settlement.reset(state.civ.seed);
      settlement.bootstrap();
      viewport.fit(settlement.world);
      app.setNote(`${settlement.name} founded`);
      if (currentPanel && currentPanel.redraw) currentPanel.redraw();
      app.requestSave();
    }, 20);
  },
  setNote(text) {
    setSaveNote(text);
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

function activeMode() {
  return MODES.find((m) => m.id === mode) || MODES[0];
}

function activeSim() {
  return mode === 'settlement' && settlement ? settlement : sim;
}

function activeSimConfig() {
  return mode === 'settlement' ? state.civ.sim : state.sim;
}

// ---- modes and tabs ------------------------------------------------------

const modesNode = document.getElementById('modes');
const tabsNode = document.getElementById('tabs');
const panelBody = document.getElementById('panel-body');
const toolbar = document.getElementById('stage-toolbar');
const status = document.getElementById('statusbar');
const saveNote = document.getElementById('save-note');

function setSaveNote(text) {
  if (saveNote) saveNote.textContent = text;
}

function showMode(id) {
  mode = id;
  const def = activeMode();
  clear(modesNode);
  for (const m of MODES) {
    modesNode.appendChild(
      el('button', {
        class: `mode${m.id === id ? ' active' : ''}`,
        type: 'button',
        text: m.label,
        onclick: () => showMode(m.id),
      }),
    );
  }
  if (id === 'settlement' && !settlement) {
    setSaveNote('growing the wilderness...');
    settlement = new Settlement(state);
    // Same trick as a restart: paint the note, then run the warmup.
    setTimeout(() => {
      settlement.bootstrap();
      viewport.fit(settlement.world);
      setSaveNote(`${settlement.name} founded`);
      if (currentPanel && currentPanel.redraw) currentPanel.redraw();
    }, 20);
  }
  buildToolbar();
  showTab(def.tabs[0].id);
  viewport.fit(activeSim().world);
}

function showTab(id) {
  const def = activeMode();
  const tab = def.tabs.find((t) => t.id === id) || def.tabs[0];
  app.ui.tab = tab.id;
  clear(tabsNode);
  for (const t of def.tabs) {
    tabsNode.appendChild(
      el('button', {
        class: `tab${t.id === tab.id ? ' active' : ''}`,
        type: 'button',
        text: t.label,
        onclick: () => showTab(t.id),
      }),
    );
  }
  currentPanel = tab.build(panelBody, app);
}

// ---- stage toolbar -------------------------------------------------------

function buildToolbar() {
  clear(toolbar);
  const cfg = activeSimConfig();

  const playBtn = button('Play', () => {
    cfg.running = !cfg.running;
    playBtn.textContent = cfg.running ? 'Pause' : 'Play';
    app.requestSave();
  });
  playBtn.textContent = cfg.running ? 'Pause' : 'Play';
  toolbar.playBtn = playBtn;

  const speedInput = el('input', { type: 'range', min: 0.25, max: 32, step: 0.25, value: cfg.speed });
  const speedLabel = el('span', { class: 'readout', text: `${cfg.speed}x` });
  speedInput.addEventListener('input', () => {
    cfg.speed = Number(speedInput.value);
    speedLabel.textContent = `${cfg.speed}x`;
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
  zoomSync = () => {
    zoomInput.value = clamp(viewport.zoom, 0.5, 16);
    zoomLabel.textContent = `${viewport.zoom.toFixed(2)}x`;
  };

  const gridToggle = el('input', { type: 'checkbox' });
  gridToggle.checked = viewport.showGrid;
  gridToggle.addEventListener('change', () => {
    viewport.showGrid = gridToggle.checked;
  });
  const occToggle = el('input', { type: 'checkbox' });
  occToggle.checked = viewport.showOccupancy;
  occToggle.addEventListener('change', () => {
    viewport.showOccupancy = occToggle.checked;
  });

  const controls = [
    playBtn,
    button('Step', () => activeSim().step(1 / cfg.tickHz)),
  ];
  if (mode === 'settlement') {
    controls.push(button('New settlers', () => app.civRestart()));
    controls.push(
      button('New land', () => {
        state.civ.seed = (Math.random() * 1e9) | 0;
        app.civRestart();
        app.rebuildPanel();
      }),
    );
  } else {
    controls.push(button('Restart', () => app.restart()));
  }
  controls.push(
    el('label', { class: 'inline' }, [el('span', { text: 'Speed' }), speedInput, speedLabel]),
    el('label', { class: 'inline' }, [el('span', { text: 'Zoom' }), zoomInput, zoomLabel]),
    button('Fit', () => {
      viewport.fit(activeSim().world);
      zoomSync();
    }),
    el('label', { class: 'inline' }, [el('span', { text: 'Grid' }), gridToggle]),
    el('label', { class: 'inline' }, [el('span', { text: 'Occupancy' }), occToggle]),
  );
  if (mode === 'settlement') {
    const labelToggle = el('input', { type: 'checkbox' });
    labelToggle.checked = !!state.civ.view.labels;
    labelToggle.addEventListener('change', () => {
      state.civ.view.labels = labelToggle.checked;
      app.requestSave();
    });
    controls.push(el('label', { class: 'inline' }, [el('span', { text: 'Labels' }), labelToggle]));
  }
  toolbar.appendChild(el('div', { class: 'toolbar-row' }, controls));
}

let zoomSync = () => {};

// ---- canvas interaction --------------------------------------------------

canvas.addEventListener('wheel', (e) => {
  e.preventDefault();
  viewport.zoomAt(e.clientX, e.clientY, e.deltaY < 0 ? 1.12 : 1 / 1.12);
  zoomSync();
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
    if (toolbar.playBtn) toolbar.playBtn.click();
  } else if (e.key === '.') {
    activeSim().step(1 / activeSimConfig().tickHz);
  } else if (e.key === 'f') {
    viewport.fit(activeSim().world);
    zoomSync();
  } else if (e.key === 'm') {
    showMode(mode === 'lab' ? 'settlement' : 'lab');
  }
});

// ---- project actions -----------------------------------------------------

document.getElementById('btn-new').addEventListener('click', () => {
  const fresh = createState();
  Object.assign(state, fresh);
  invalidateSamplerCache();
  sim.env.invalidate();
  sim.reset(state.seed);
  settlement = null;
  viewport.fit(sim.world);
  app.ui.selectedSamplerId = state.materials.samplers[0].id;
  app.ui.selectedSpeciesId = state.species[0].id;
  showMode('lab');
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
    settlement = null;
    viewport.fit(sim.world);
    app.ui.selectedSamplerId = state.materials.samplers[0].id;
    app.ui.selectedSpeciesId = state.species[0].id;
    showMode('lab');
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

  const active = activeSim();
  const cfg = activeSimConfig();
  if (cfg.running && (mode !== 'settlement' || (settlement && settlement.ready))) {
    const stepDt = 1 / cfg.tickHz;
    accumulator += dtReal * cfg.speed;
    let steps = 0;
    while (accumulator >= stepDt && steps < 400) {
      active.step(stepDt);
      accumulator -= stepDt;
      steps++;
    }
    if (accumulator > 2) accumulator = 0;
  } else {
    accumulator = 0;
  }

  active.processRasterQueue(cfg.rasterBudget);
  viewport.draw(active);
  if (currentPanel && currentPanel.tick) currentPanel.tick(dtReal);
  updateStatus();
  requestAnimationFrame(frame);
}

function updateStatus() {
  if (mode === 'settlement') {
    status.textContent = settlement && settlement.ready ? settlementStatus() : 'growing the wilderness...';
    return;
  }
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

function settlementStatus() {
  const s = settlement.stats();
  const clock = `${String(Math.floor(s.dayFraction * 24)).padStart(2, '0')}:${String(
    Math.floor((s.dayFraction * 24 * 60) % 60),
  ).padStart(2, '0')}`;
  const jobs = Object.entries(s.professions)
    .filter(([id]) => id !== 'child')
    .map(([id, n]) => `${PROFESSIONS[id] || id} ${n}`)
    .join(' ');
  return [
    settlement.name,
    `day ${s.day} ${clock}`,
    `people ${s.population} (${s.children} children)`,
    `beds ${s.housing}`,
    `built ${s.buildings}${s.sites ? ` +${s.sites}` : ''}`,
    `food ${Math.round(settlement.stock.food || 0)}`,
    `coin ${Math.round(s.coin)}`,
    `tech ${s.known}/${s.techs}`,
    jobs,
    `${fps.toFixed(0)} fps`,
  ].join('   ');
}

// ---- boot ----------------------------------------------------------------

const resizeObserver = new ResizeObserver(() => {
  viewport.resize();
});
resizeObserver.observe(canvas.parentElement);

showMode('lab');
viewport.fit(sim.world);
requestAnimationFrame(frame);
