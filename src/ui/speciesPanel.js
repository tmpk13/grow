// Species panel: the parameter form (generated from SPECIES_SCHEMA) plus an
// isolated growth preview for the selected species.

import {
  boolField,
  button,
  clear,
  el,
  numberField,
  rangeField,
  section,
  selectField,
  textField,
} from './controls.js';
import {
  SIZE_CLASSES,
  SPECIES_SCHEMA,
  effectiveLimits,
  getPath,
  makeSpecies,
  setPath,
} from '../species.js';
import { makePreviewPlant } from '../sim.js';
import { drawPlantPreview } from '../render.js';
import { uid } from '../util.js';

export function buildSpeciesPanel(root, app) {
  const state = app.state;
  clear(root);

  if (!app.ui.selectedSpeciesId || !state.species.some((s) => s.id === app.ui.selectedSpeciesId)) {
    app.ui.selectedSpeciesId = state.species[0]?.id;
  }
  const species = state.species.find((s) => s.id === app.ui.selectedSpeciesId);

  const chips = el('div', { class: 'chips' });
  for (const sp of state.species) {
    chips.appendChild(
      el('button', {
        class: `chip${sp.id === app.ui.selectedSpeciesId ? ' active' : ''}${sp.enabled ? '' : ' off'}`,
        type: 'button',
        text: sp.name,
        onclick: () => {
          app.ui.selectedSpeciesId = sp.id;
          app.rebuildPanel();
        },
      }),
    );
  }

  const listActions = el('div', { class: 'btn-row' }, [
    button('Add', () => {
      const sp = makeSpecies({ name: `Species ${state.species.length + 1}` });
      state.species.push(sp);
      app.ui.selectedSpeciesId = sp.id;
      app.speciesChanged();
      app.rebuildPanel();
    }),
    button('Duplicate', () => {
      if (!species) return;
      const copy = makeSpecies({ ...JSON.parse(JSON.stringify(species)), id: uid('sp'), name: `${species.name} copy` });
      state.species.push(copy);
      app.ui.selectedSpeciesId = copy.id;
      app.speciesChanged();
      app.rebuildPanel();
    }),
    button('Remove', () => {
      if (!species || state.species.length <= 1) return;
      const i = state.species.indexOf(species);
      state.species.splice(i, 1);
      app.ui.selectedSpeciesId = state.species[Math.max(0, i - 1)].id;
      app.speciesChanged();
      app.rebuildPanel();
    }, 'danger'),
  ]);

  root.appendChild(section('Species', [chips, listActions]));

  if (!species) return { tick() {} };

  // ---- preview ----------------------------------------------------------
  const previewCanvas = el('canvas', { class: 'species-preview' });
  const previewInfo = el('p', { class: 'note' });
  let previewSeed = 1234;
  let plant = makePreviewPlant(state, species, previewSeed);
  let previewSpeed = 4;
  let previewPaused = false;

  const resetPreview = (seed) => {
    previewSeed = seed ?? (Math.random() * 1e9) | 0;
    plant = makePreviewPlant(state, species, previewSeed);
    plant.raster(app.env);
    drawPlantPreview(previewCanvas, plant, 0);
  };

  const previewActions = el('div', { class: 'btn-row' }, [
    button('Regrow', () => resetPreview()),
    button('Grow to full', () => {
      let guard = 0;
      while (!plant.mature && guard < 4000) {
        plant.grow(1, plant.previewCtx);
        guard++;
      }
      plant.raster(app.env);
      drawPlantPreview(previewCanvas, plant, 0);
    }),
    button('Pause', (e) => {
      previewPaused = !previewPaused;
      e.target.textContent = previewPaused ? 'Resume' : 'Pause';
    }),
  ]);

  const previewSpeedField = numberField('Preview speed', {
    value: previewSpeed,
    min: 0.25,
    max: 40,
    step: 0.25,
    onInput: (v) => {
      previewSpeed = v;
    },
  });

  const eff = effectiveLimits(species, state.classLimits);
  const effNote = el('p', {
    class: 'note',
    text:
      `Effective limits after the ${SIZE_CLASSES[species.sizeClass].label} class ceiling: ` +
      `radius ${eff.maxRadiusCells} cells, height ${eff.maxHeightPx} px, ` +
      `spacing ${eff.minSpacing} cells, max ${eff.maxInstances} instances.`,
  });

  root.appendChild(
    section('Growth preview', [
      el('div', { class: 'preview-wrap tall' }, [previewCanvas]),
      previewActions,
      previewSpeedField,
      previewInfo,
      effNote,
    ]),
  );

  // ---- parameter form ---------------------------------------------------
  for (const group of SPECIES_SCHEMA) {
    const fields = group.fields.map((f) => buildField(f, species, app));
    root.appendChild(section(group.group, fields));
  }

  resetPreview(previewSeed);

  return {
    tick(dt) {
      if (previewPaused) return;
      if (!plant.mature) {
        plant.grow(dt * previewSpeed, plant.previewCtx);
        if (plant.dirty) plant.raster(app.env);
        drawPlantPreview(previewCanvas, plant, 0);
      }
      previewInfo.textContent =
        `age ${plant.age.toFixed(0)}, segments ${plant.segments.length}, leaves ${plant.leaves.length}, ` +
        `active tips ${plant.aliveTipCount}${plant.mature ? ' (mature)' : ''}`;
    },
    redraw() {
      plant.raster(app.env);
      drawPlantPreview(previewCanvas, plant, 0);
    },
  };
}

function buildField(f, species, app) {
  const commit = () => {
    app.speciesChanged();
  };
  if (f.type === 'range') {
    return rangeField(f.label, {
      minValue: getPath(species, f.pathMin),
      maxValue: getPath(species, f.pathMax),
      min: f.min,
      max: f.max,
      step: f.step,
      hint: f.hint,
      onInput: (lo, hi) => {
        setPath(species, f.pathMin, lo);
        setPath(species, f.pathMax, hi);
        commit();
      },
    });
  }
  if (f.type === 'number') {
    return numberField(f.label, {
      value: getPath(species, f.path),
      min: f.min,
      max: f.max,
      step: f.step,
      hint: f.hint,
      onInput: (v) => {
        setPath(species, f.path, v);
        commit();
      },
    });
  }
  if (f.type === 'bool') {
    return boolField(f.label, {
      value: getPath(species, f.path),
      hint: f.hint,
      onInput: (v) => {
        setPath(species, f.path, v);
        commit();
      },
    });
  }
  if (f.type === 'text') {
    return textField(f.label, {
      value: getPath(species, f.path),
      hint: f.hint,
      onInput: (v) => {
        setPath(species, f.path, v);
        app.requestSave();
      },
    });
  }
  if (f.type === 'select') {
    const options = f.options.map((o) => ({ value: o, label: SIZE_CLASSES[o] ? SIZE_CLASSES[o].label : o }));
    return selectField(f.label, {
      value: getPath(species, f.path),
      options,
      hint: f.hint,
      onInput: (v) => {
        setPath(species, f.path, v);
        commit();
        app.rebuildPanel();
      },
    });
  }
  if (f.type === 'sampler') {
    return selectField(f.label, {
      value: getPath(species, f.path),
      options: app.state.materials.samplers.map((s) => ({ value: s.id, label: s.name })),
      hint: f.hint,
      onInput: (v) => {
        setPath(species, f.path, v);
        commit();
      },
    });
  }
  return el('div', { class: 'note', text: `unsupported field ${f.type}` });
}
