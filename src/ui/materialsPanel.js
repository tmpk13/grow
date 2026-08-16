// Materials panel: the sampling boxes, their layout mode and the pixel editor.

import { button, clear, el, numberField, row, section, selectField, textField } from './controls.js';
import { GridEditor } from './gridEditor.js';
import {
  ROLES,
  ROLE_LABELS,
  copyAtlasToSamplers,
  createSampler,
  fillDefaultArt,
  findSampler,
  invalidateSamplerCache,
  paintAtlasFromSamplers,
  resizeSampler,
  samplerPatch,
  samplerRamp,
} from '../sampler.js';
import { EMPTY_COLOR, hexToPacked, mixPacked, packedToHex, uid } from '../util.js';

const TOOLS = [
  { id: 'pencil', label: 'Pencil' },
  { id: 'eraser', label: 'Eraser' },
  { id: 'fill', label: 'Fill' },
  { id: 'pick', label: 'Pick' },
];

export function buildMaterialsPanel(root, app) {
  const state = app.state;
  const materials = state.materials;
  clear(root);

  if (!app.ui.selectedSamplerId) app.ui.selectedSamplerId = materials.samplers[0]?.id;
  const selected = findSampler(materials, app.ui.selectedSamplerId) || materials.samplers[0];

  const editorWrap = el('div', { class: 'editor-wrap' });
  const canvas = el('canvas', { class: 'grid-canvas' });
  editorWrap.appendChild(canvas);

  const gridFor = () =>
    materials.mode === 'single'
      ? { w: materials.atlas.w, h: materials.atlas.h, px: materials.atlas.px }
      : selected
        ? { w: selected.w, h: selected.h, px: selected.px }
        : null;

  const setAspect = () => {
    const g = gridFor();
    if (g) editorWrap.style.aspectRatio = `${g.w} / ${g.h}`;
  };
  setAspect();

  const editor = new GridEditor(canvas, {
    getGrid: gridFor,
    onCommit: () => {
      app.materialsChanged();
      drawRamp();
      drawThumbs();
    },
    getOverlays: () => {
      if (materials.mode !== 'single') return [];
      return materials.samplers.map((s) => ({
        x: s.region.x,
        y: s.region.y,
        w: s.region.w,
        h: s.region.h,
        color: s.id === (selected && selected.id) ? 'rgba(255,210,120,0.95)' : 'rgba(255,255,255,0.35)',
        active: s.id === (selected && selected.id),
        label: s.name,
      }));
    },
  });
  editor.color = app.ui.brushColor ?? hexToPacked('#7ab55c');
  editor.tool = app.ui.tool || 'pencil';
  editor.mirrorX = !!app.ui.mirrorX;
  editor.onPick = (v) => {
    if (v === EMPTY_COLOR) return;
    app.ui.brushColor = v;
    editor.color = v;
    colorInput.value = packedToHex(v);
  };
  app.gridEditor = editor;

  // ---- mode -------------------------------------------------------------
  const modeRow = selectField('Grid layout', {
    value: materials.mode,
    options: [
      { value: 'multi', label: 'Separate box per material' },
      { value: 'single', label: 'One shared grid' },
    ],
    onInput: (v) => {
      if (v === materials.mode) return;
      if (v === 'single') paintAtlasFromSamplers(materials);
      materials.mode = v;
      app.materialsChanged();
      app.rebuildPanel();
    },
    hint: 'switching to one grid copies the boxes into it',
  });

  const syncButtons = el('div', { class: 'btn-row' }, [
    button('Boxes to shared grid', () => {
      paintAtlasFromSamplers(materials);
      app.materialsChanged();
      app.rebuildPanel();
    }),
    button('Shared grid to boxes', () => {
      copyAtlasToSamplers(materials);
      app.materialsChanged();
      app.rebuildPanel();
    }),
  ]);

  // ---- tools ------------------------------------------------------------
  const toolButtons = el('div', { class: 'btn-row' });
  for (const t of TOOLS) {
    const b = button(t.label, () => {
      editor.tool = t.id;
      app.ui.tool = t.id;
      for (const child of toolButtons.children) child.classList.toggle('active', child === b);
    });
    if (editor.tool === t.id) b.classList.add('active');
    toolButtons.appendChild(b);
  }

  const colorInput = el('input', { type: 'color', value: packedToHex(editor.color) });
  colorInput.addEventListener('input', () => {
    editor.color = hexToPacked(colorInput.value);
    app.ui.brushColor = editor.color;
  });

  const mirrorBox = el('input', { type: 'checkbox' });
  mirrorBox.checked = editor.mirrorX;
  mirrorBox.addEventListener('change', () => {
    editor.mirrorX = mirrorBox.checked;
    app.ui.mirrorX = mirrorBox.checked;
  });

  const swatches = el('div', { class: 'swatches' });
  const drawSwatches = () => {
    clear(swatches);
    const ramp = selected ? samplerRamp(materials, selected) : [];
    for (const c of ramp.slice().reverse()) {
      const sw = el('button', {
        class: 'swatch',
        type: 'button',
        title: packedToHex(c),
        style: { background: packedToHex(c) },
        onclick: () => {
          editor.color = c;
          app.ui.brushColor = c;
          colorInput.value = packedToHex(c);
        },
      });
      swatches.appendChild(sw);
    }
  };

  const rampA = el('input', { type: 'color', value: '#1d2b1a' });
  const rampB = el('input', { type: 'color', value: '#9ed07a' });
  const rampRow = el('div', { class: 'btn-row' }, [
    rampA,
    rampB,
    button('Make ramp', () => {
      const g = materials.mode === 'single' && selected ? regionGrid(materials, selected) : gridFor();
      if (!g) return;
      fillRamp(g, hexToPacked(rampA.value), hexToPacked(rampB.value));
      if (materials.mode === 'single' && selected) writeRegion(materials, selected, g);
      app.materialsChanged();
      editor.draw();
      drawRamp();
      drawThumbs();
      drawSwatches();
    }),
    button('Clear', () => {
      const g = materials.mode === 'single' && selected ? regionGrid(materials, selected) : gridFor();
      if (!g) return;
      g.px.fill(EMPTY_COLOR);
      if (materials.mode === 'single' && selected) writeRegion(materials, selected, g);
      app.materialsChanged();
      editor.draw();
      drawRamp();
      drawThumbs();
      drawSwatches();
    }),
  ]);

  // ---- resolved ramp ----------------------------------------------------
  const rampStrip = el('div', { class: 'ramp-strip' });
  const rampNote = el('p', { class: 'note' });
  const drawRamp = () => {
    clear(rampStrip);
    const ramp = selected ? samplerRamp(materials, selected) : [];
    for (const c of ramp) {
      rampStrip.appendChild(el('span', { class: 'ramp-cell', style: { background: packedToHex(c) } }));
    }
    rampNote.textContent = selected
      ? `${ramp.length} tones, dark to light. Shading picks along this ramp.`
      : 'No sampler selected.';
  };

  // ---- sampler list -----------------------------------------------------
  const list = el('div', { class: 'sampler-list' });
  const thumbs = [];
  const drawThumbs = () => {
    for (const { canvas: tc, sampler } of thumbs) drawThumb(tc, materials, sampler);
  };

  const rebuildList = () => {
    clear(list);
    thumbs.length = 0;
    for (const s of materials.samplers) {
      const tc = el('canvas', { class: 'thumb' });
      thumbs.push({ canvas: tc, sampler: s });
      const item = el(
        'button',
        {
          class: `sampler-item${selected && s.id === selected.id ? ' active' : ''}`,
          type: 'button',
          onclick: () => {
            app.ui.selectedSamplerId = s.id;
            app.rebuildPanel();
          },
        },
        [
          tc,
          el('span', { class: 'sampler-meta' }, [
            el('strong', { text: s.name }),
            el('span', { text: ROLE_LABELS[s.role] || s.role }),
          ]),
        ],
      );
      list.appendChild(item);
    }
    requestAnimationFrame(drawThumbs);
  };

  const listActions = [
    button('Add box', () => {
      const bandY = materials.samplers.length * 2 % Math.max(1, materials.atlas.h);
      const s = createSampler({
        id: uid('mat'),
        name: `Box ${materials.samplers.length + 1}`,
        role: 'leaf',
        w: 16,
        h: 6,
        region: { x: 0, y: bandY, w: materials.atlas.w, h: 2 },
      });
      fillDefaultArt(s, ROLES.find((r) => r.id === 'leaf'), materials.samplers.length * 17);
      materials.samplers.push(s);
      app.ui.selectedSamplerId = s.id;
      app.materialsChanged();
      app.rebuildPanel();
    }),
    button('Remove', () => {
      if (!selected || materials.samplers.length <= 1) return;
      const i = materials.samplers.indexOf(selected);
      materials.samplers.splice(i, 1);
      app.ui.selectedSamplerId = materials.samplers[Math.max(0, i - 1)].id;
      app.materialsChanged();
      app.rebuildPanel();
    }, 'danger'),
  ];

  // ---- selected sampler settings ---------------------------------------
  const settings = el('div', {});
  if (selected) {
    settings.appendChild(
      textField('Name', {
        value: selected.name,
        onInput: (v) => {
          selected.name = v;
          rebuildList();
          app.requestSave();
        },
      }),
    );
    settings.appendChild(
      selectField('Role', {
        value: selected.role,
        options: ROLES.map((r) => ({ value: r.id, label: r.label })),
        onInput: (v) => {
          selected.role = v;
          rebuildList();
          app.requestSave();
        },
        hint: 'a label only; species pick boxes per material slot',
      }),
    );
    if (materials.mode === 'multi') {
      settings.appendChild(
        numberField('Box width', {
          value: selected.w,
          min: 1,
          max: 64,
          step: 1,
          onInput: (v) => {
            resizeSampler(selected, Math.max(1, v | 0), selected.h);
            setAspect();
            app.materialsChanged();
            editor.draw();
            drawRamp();
            drawThumbs();
          },
        }),
      );
      settings.appendChild(
        numberField('Box height', {
          value: selected.h,
          min: 1,
          max: 64,
          step: 1,
          onInput: (v) => {
            resizeSampler(selected, selected.w, Math.max(1, v | 0));
            setAspect();
            app.materialsChanged();
            editor.draw();
            drawRamp();
            drawThumbs();
          },
        }),
      );
    } else {
      const regionField = (label, key, max) =>
        numberField(label, {
          value: selected.region[key],
          min: 0,
          max,
          step: 1,
          onInput: (v) => {
            selected.region[key] = Math.max(key === 'w' || key === 'h' ? 1 : 0, v | 0);
            invalidateSamplerCache();
            materials.version++;
            app.materialsChanged();
            editor.draw();
            drawRamp();
            drawThumbs();
          },
        });
      settings.appendChild(regionField('Region x', 'x', materials.atlas.w - 1));
      settings.appendChild(regionField('Region y', 'y', materials.atlas.h - 1));
      settings.appendChild(regionField('Region width', 'w', materials.atlas.w));
      settings.appendChild(regionField('Region height', 'h', materials.atlas.h));
    }
  }

  const atlasSettings = el('div', {});
  if (materials.mode === 'single') {
    const resizeAtlas = (w, h) => {
      const next = new Uint32Array(w * h);
      for (let y = 0; y < Math.min(h, materials.atlas.h); y++) {
        for (let x = 0; x < Math.min(w, materials.atlas.w); x++) {
          next[y * w + x] = materials.atlas.px[y * materials.atlas.w + x];
        }
      }
      materials.atlas = { w, h, px: next };
      invalidateSamplerCache();
      materials.version++;
      setAspect();
      app.materialsChanged();
      app.rebuildPanel();
    };
    atlasSettings.appendChild(
      numberField('Shared grid width', {
        value: materials.atlas.w,
        min: 2,
        max: 128,
        step: 1,
        onInput: (v) => resizeAtlas(Math.max(2, v | 0), materials.atlas.h),
      }),
    );
    atlasSettings.appendChild(
      numberField('Shared grid height', {
        value: materials.atlas.h,
        min: 2,
        max: 128,
        step: 1,
        onInput: (v) => resizeAtlas(materials.atlas.w, Math.max(2, v | 0)),
      }),
    );
  }

  root.appendChild(
    section('Sampling grid', [
      modeRow,
      syncButtons,
      atlasSettings,
      toolButtons,
      row('Brush color', el('span', { class: 'inline' }, [colorInput])),
      row('Mirror X', mirrorBox),
      swatches,
      editorWrap,
      rampRow,
      rampStrip,
      rampNote,
    ]),
  );
  root.appendChild(section('Boxes', [list, el('div', { class: 'btn-row' }, listActions), settings]));

  rebuildList();
  drawRamp();
  drawSwatches();
  requestAnimationFrame(() => editor.draw());

  return {
    redraw() {
      editor.draw();
      drawRamp();
      drawThumbs();
      drawSwatches();
    },
  };
}

// A detached copy of a sampler's atlas region, so bulk operations in single
// grid mode only touch that region.
function regionGrid(materials, sampler) {
  const patch = samplerPatch(materials, sampler);
  return { w: patch.w, h: patch.h, px: patch.px };
}

function writeRegion(materials, sampler, grid) {
  const { atlas } = materials;
  const r = sampler.region;
  for (let y = 0; y < grid.h; y++) {
    const ay = r.y + y;
    if (ay < 0 || ay >= atlas.h) continue;
    for (let x = 0; x < grid.w; x++) {
      const ax = r.x + x;
      if (ax < 0 || ax >= atlas.w) continue;
      atlas.px[ay * atlas.w + ax] = grid.px[y * grid.w + x];
    }
  }
}

function fillRamp(grid, dark, light) {
  for (let y = 0; y < grid.h; y++) {
    for (let x = 0; x < grid.w; x++) {
      const t = grid.w > 1 ? x / (grid.w - 1) : 0;
      const vy = grid.h > 1 ? (y / (grid.h - 1) - 0.5) * 0.12 : 0;
      grid.px[y * grid.w + x] = mixPacked(dark, light, Math.min(1, Math.max(0, t + vy)));
    }
  }
}

function drawThumb(canvas, materials, sampler) {
  const patch = samplerPatch(materials, sampler);
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(rect.width * dpr));
  const h = Math.max(1, Math.round(rect.height * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, rect.width, rect.height);
  const cw = rect.width / patch.w;
  const ch = rect.height / patch.h;
  for (let y = 0; y < patch.h; y++) {
    for (let x = 0; x < patch.w; x++) {
      const v = patch.px[y * patch.w + x];
      ctx.fillStyle = v === EMPTY_COLOR ? '#12161c' : packedToHex(v);
      ctx.fillRect(x * cw, y * ch, Math.ceil(cw), Math.ceil(ch));
    }
  }
}
