// Land panel: the settlement map, the terrain that generates it and what the
// view draws on top.

import {
  boolField,
  button,
  clear,
  colorField,
  el,
  numberField,
  section,
  selectField,
} from './controls.js';
import { DEPOSIT_KINDS } from '../civ/terrain.js';

export function buildLandPanel(root, app) {
  const civ = app.state.civ;
  clear(root);

  const mapNum = (label, key, min, max, step, hint) =>
    numberField(label, {
      value: civ.world[key],
      min,
      max,
      step,
      hint,
      onInput: (v) => {
        civ.world[key] = v | 0;
        app.civRestart();
      },
    });

  root.appendChild(
    section('Map', [
      mapNum('Columns (x)', 'cols', 24, 300, 1, 'cells across the map'),
      mapNum('Rows (depth)', 'rows', 12, 160, 1),
      mapNum('Cell width (px)', 'cellPx', 4, 24, 1, 'everything drawn is sized from this'),
      mapNum('Cell depth (px)', 'depthPx', 2, 24, 1, 'lower values tilt the ground toward the viewer'),
      mapNum('Sky height (px)', 'skyPx', 20, 400, 2),
      numberField('Seed', {
        value: civ.seed,
        min: 1,
        max: 999999999,
        step: 1,
        hint: 'terrain, deposits, people and everything they do',
        onInput: (v) => {
          civ.seed = v | 0;
          app.requestSave();
        },
      }),
      el('div', { class: 'btn-row' }, [
        button('New land', () => {
          civ.seed = (Math.random() * 1e9) | 0;
          app.civRestart();
          app.rebuildPanel();
        }),
        button('Rebuild this land', () => app.civRestart()),
      ]),
      el('p', { class: 'note', text: 'Any change here regenerates the map and restarts the settlement.' }),
    ]),
  );

  const terrainNum = (label, key, min, max, step, hint) =>
    numberField(label, {
      value: civ.terrain[key],
      min,
      max,
      step,
      hint,
      onInput: (v) => {
        civ.terrain[key] = v;
        app.civRestart();
      },
    });

  root.appendChild(
    section('Terrain', [
      terrainNum('Feature size', 'scale', 4, 48, 1, 'cells per noise feature; larger means broader hills and lakes'),
      terrainNum('Octaves', 'octaves', 1, 6, 1),
      terrainNum('Roughness', 'persistence', 0.15, 0.85, 0.05),
      terrainNum('Warp', 'warp', 0, 1.2, 0.05, 'bends the coastlines out of the noise grid'),
      terrainNum('Water level', 'waterLevel', 0, 0.7, 0.01),
      terrainNum('Shore width', 'sandBand', 0, 0.2, 0.01),
      terrainNum('Rock level', 'rockLevel', 0.3, 1, 0.01),
      terrainNum('Moisture scale', 'moistScale', 4, 60, 1),
      terrainNum('Fertility', 'fertility', 0, 1.5, 0.05, 'how much the damp ground feeds a farm'),
      terrainNum('Wild growth', 'wildness', 0.2, 6, 0.1,
        'how lush the map is: scales seeding and how many plants the land carries'),
      terrainNum('Wilderness warmup (s)', 'warmup', 0, 3000, 30,
        'growth simulated before the people arrive'),
    ]),
  );

  const depositFields = [];
  for (const kind of DEPOSIT_KINDS) {
    const cfg = civ.terrain.deposits[kind];
    const num = (label, key, min, max, step) =>
      numberField(label, {
        value: cfg[key],
        min,
        max,
        step,
        onInput: (v) => {
          cfg[key] = v;
          app.civRestart();
        },
      });
    depositFields.push(
      el('div', { class: 'class-block' }, [
        el('h4', { text: kind }),
        num('Clusters per 100 cells', 'density', 0, 5, 0.05),
        num('Cluster cells (min)', 'clusterMin', 1, 12, 1),
        num('Cluster cells (max)', 'clusterMax', 1, 20, 1),
        num('Amount per cell (min)', 'amountMin', 5, 900, 5),
        num('Amount per cell (max)', 'amountMax', 5, 2000, 5),
      ]),
    );
  }
  root.appendChild(
    section('Deposits', [
      el('p', { class: 'note', text:
        'Stone and ore sit in the high rock, clay along the water. Every deposit holds a finite ' +
        'amount, so a settlement that has emptied the ground near it has to reach further out.' }),
      ...depositFields,
    ]),
  );

  const viewToggle = (label, key, hint) =>
    boolField(label, {
      value: civ.view[key] !== false,
      hint,
      onInput: (v) => {
        civ.view[key] = v;
        app.civRepaint();
      },
    });

  root.appendChild(
    section('View', [
      viewToggle('Day and night', 'dayNight', 'tints the map with the hour and lights the windows'),
      viewToggle('Footpaths', 'paths', 'cells that get walked over wear into a path'),
      viewToggle('Deposits', 'deposits'),
      viewToggle('People', 'people'),
      viewToggle('Chimney smoke', 'smoke'),
      boolField('Building labels', {
        value: !!civ.view.labels,
        onInput: (v) => {
          civ.view.labels = v;
          app.civRepaint();
        },
      }),
      colorField('Shallow water', {
        value: civ.view.waterTop,
        onInput: (v) => {
          civ.view.waterTop = v;
          app.civRepaint();
        },
      }),
      colorField('Deep water', {
        value: civ.view.waterDeep,
        onInput: (v) => {
          civ.view.waterDeep = v;
          app.civRepaint();
        },
      }),
      colorField('Footpath', {
        value: civ.view.pathColor,
        onInput: (v) => {
          civ.view.pathColor = v;
          app.civRepaint();
        },
      }),
      selectField('Soil texture', {
        value: civ.world.soilSampler,
        options: app.state.materials.samplers.map((s) => ({ value: s.id, label: s.name })),
        onInput: (v) => {
          civ.world.soilSampler = v;
          app.civRepaint();
        },
      }),
    ]),
  );

  const summary = el('div', { class: 'stat-grid' });
  root.appendChild(section('This land', [summary]));

  const redraw = () => {
    const sim = app.civ;
    clear(summary);
    if (!sim || !sim.terrain) return;
    const cells = sim.world.cols * sim.world.rows;
    const rows = [
      ['Name', sim.name],
      ['Cells', `${sim.world.cols} x ${sim.world.rows}`],
      ['Water', `${Math.round((sim.terrain.waterCells / cells) * 100)}%`],
      ['Plants', String(sim.plantSim.plants.length)],
    ];
    for (const kind of DEPOSIT_KINDS) {
      const d = sim.terrain.countDeposits(kind);
      rows.push([`${kind} left`, `${Math.round(d.amount)} in ${d.cells} spots`]);
    }
    for (const [k, v] of rows) {
      summary.appendChild(el('div', { class: 'stat' }, [
        el('span', { class: 'stat-key', text: k }),
        el('span', { class: 'stat-val', text: v }),
      ]));
    }
  };
  redraw();

  let since = 0;
  return {
    redraw,
    tick(dt) {
      since += dt;
      if (since < 1) return;
      since = 0;
      redraw();
    },
  };
}
