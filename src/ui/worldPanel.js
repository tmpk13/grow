// World panel: grid size, per size class ceilings and simulation settings.

import { button, clear, colorField, el, numberField, section, selectField } from './controls.js';
import { SIZE_CLASSES } from '../species.js';

export function buildWorldPanel(root, app) {
  const state = app.state;
  clear(root);

  const worldNum = (label, key, min, max, step, hint) =>
    numberField(label, {
      value: state.world[key],
      min,
      max,
      step,
      hint,
      onInput: (v) => {
        state.world[key] = v | 0;
        app.worldChanged();
      },
    });

  root.appendChild(
    section('Grid', [
      worldNum('Columns', 'cols', 8, 400, 1),
      worldNum('Rows', 'rows', 8, 300, 1),
      worldNum('Cell size (px)', 'cellPx', 2, 32, 1, 'pixels per grid cell'),
      worldNum('Soil row', 'soilRow', 1, 299, 1, 'first row of soil; plants root on it'),
      colorField('Sky top', {
        value: state.world.skyTop,
        onInput: (v) => {
          state.world.skyTop = v;
          app.repaintBackground();
        },
      }),
      colorField('Sky horizon', {
        value: state.world.skyBottom,
        onInput: (v) => {
          state.world.skyBottom = v;
          app.repaintBackground();
        },
      }),
      selectField('Soil texture', {
        value: state.world.soilSampler,
        options: state.materials.samplers.map((s) => ({ value: s.id, label: s.name })),
        onInput: (v) => {
          state.world.soilSampler = v;
          app.repaintBackground();
        },
      }),
      el('p', { class: 'note', text: 'Changing grid size or cell size restarts the simulation.' }),
    ]),
  );

  const classFields = [];
  for (const [id, def] of Object.entries(SIZE_CLASSES)) {
    const limits = state.classLimits[id];
    const num = (label, key, min, max, hint) =>
      numberField(label, {
        value: limits[key],
        min,
        max,
        step: 1,
        hint,
        onInput: (v) => {
          limits[key] = v | 0;
          app.speciesChanged();
        },
      });
    classFields.push(
      el('div', { class: 'class-block' }, [
        el('h4', { text: `${def.label} (layer ${def.layer})` }),
        num('Max footprint radius (cells)', 'maxRadiusCells', 0, 30, 'ceiling on the perimeter a plant may claim'),
        num('Max height (px)', 'maxHeightPx', 4, 400),
        num('Min spacing (cells)', 'minSpacing', 0, 30, 'gap enforced between two items of this class'),
        num('Max instances', 'maxInstances', 0, 800),
      ]),
    );
  }
  root.appendChild(
    section('Size class limits', [
      el('p', { class: 'note', text:
        'One item per cell per class, so a ground cover and a tree can share a cell but two trees cannot. ' +
        'A species can never exceed its class ceiling. Plants already growing keep the limits they started with.' }),
      ...classFields,
    ]),
  );

  root.appendChild(
    section('Simulation', [
      numberField('Seed', {
        value: state.seed,
        min: 1,
        max: 999999999,
        step: 1,
        onInput: (v) => {
          state.seed = v | 0;
          app.requestSave();
        },
        hint: 'applied on restart',
      }),
      numberField('Ticks per second', {
        value: state.sim.tickHz,
        min: 1,
        max: 120,
        step: 1,
        onInput: (v) => {
          state.sim.tickHz = v | 0;
          app.requestSave();
        },
      }),
      numberField('Redraws per frame', {
        value: state.sim.rasterBudget,
        min: 1,
        max: 80,
        step: 1,
        hint: 'plants rasterized per frame; lower keeps the view responsive',
        onInput: (v) => {
          state.sim.rasterBudget = v | 0;
          app.requestSave();
        },
      }),
      el('div', { class: 'btn-row' }, [
        button('Restart', () => app.restart()),
        button('New seed and restart', () => {
          state.seed = (Math.random() * 1e9) | 0;
          app.restart();
          app.rebuildPanel();
        }),
        button('Clear plants', () => {
          app.sim.removeAll();
        }, 'danger'),
      ]),
    ]),
  );

  return { redraw() {} };
}
