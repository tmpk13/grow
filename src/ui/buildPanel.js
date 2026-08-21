// Build panel: the planner's weights, the catalog of what can be raised, and
// the sites currently going up.

import { boolField, button, clear, el, numberField, section } from './controls.js';
import { BUILDINGS, CATEGORIES, scaledCost } from '../civ/buildings.js';
import { formatCost, RES } from '../civ/resources.js';

export function buildBuildPanel(root, app) {
  const civ = app.state.civ;
  clear(root);

  const num = (label, key, min, max, step, hint) =>
    numberField(label, {
      value: civ.build[key],
      min,
      max,
      step,
      hint,
      onInput: (v) => {
        civ.build[key] = v;
        app.requestSave();
      },
    });

  root.appendChild(
    section('Planner', [
      boolField('Plan buildings automatically', {
        value: civ.build.autoBuild !== false,
        hint: 'off leaves every building to the Build buttons below',
        onInput: (v) => {
          civ.build.autoBuild = v;
          app.requestSave();
        },
      }),
      num('Sites at once', 'maxSites', 1, 12, 1, 'how many buildings may be under construction'),
      num('Spacing (cells)', 'spacing', 0, 4, 1, 'gap kept between buildings'),
      num('Sprawl (cells)', 'sprawl', 4, 80, 1, 'how far from the center a site may be'),
      num('Cost scale', 'costScale', 0.1, 4, 0.1),
      num('Work scale', 'workScale', 0.1, 4, 0.1),
      num('Housing headroom', 'housingSlack', 0, 20, 1, 'empty beds kept ahead of the population'),
    ]),
  );

  const weightFields = Object.entries(CATEGORIES).map(([id, label]) =>
    numberField(`${label} weight`, {
      value: civ.build.weights[id] ?? 1,
      min: 0,
      max: 3,
      step: 0.1,
      onInput: (v) => {
        civ.build.weights[id] = v;
        app.requestSave();
      },
    }),
  );
  const perTypeFields = Object.entries(civ.build.perType).map(([id, value]) =>
    numberField(`People per ${CATEGORIES[id] ? CATEGORIES[id].toLowerCase() : id} building`, {
      value,
      min: 1,
      max: 60,
      step: 1,
      onInput: (v) => {
        civ.build.perType[id] = v | 0;
        app.requestSave();
      },
    }),
  );
  root.appendChild(section('What to favor', [...weightFields, ...perTypeFields]));

  const sites = el('div', { class: 'roster' });
  root.appendChild(section('Under construction', [sites]));

  const catalog = el('div', { class: 'catalog' });
  root.appendChild(
    section('Catalog', [
      el('p', { class: 'note', text:
        'Build places a site; the materials still have to be carried there before anyone can raise it.' }),
      catalog,
    ]),
  );

  const redraw = () => {
    const sim = app.civ;
    clear(sites);
    clear(catalog);
    if (!sim) return;

    for (const site of sim.sites) {
      const cost = site.cost;
      const missing = Object.entries(cost)
        .map(([id, n]) => [id, n - (site.delivered[id] || 0)])
        .filter(([, n]) => n > 0);
      const progress = site.workDone / Math.max(1, site.work);
      sites.appendChild(
        el('div', { class: 'roster-row' }, [
          el('span', { class: 'roster-name', text: site.def.label }),
          el('span', { class: 'roster-task', text: missing.length
            ? `waiting on ${missing.map(([id, n]) => `${Math.ceil(n)} ${RES[id].label.toLowerCase()}`).join(', ')}`
            : `raising ${Math.round(progress * 100)}%` }),
          barOf(progress),
        ]),
      );
    }
    if (!sim.sites.length) sites.appendChild(el('p', { class: 'note', text: 'Nothing under construction.' }));

    for (const [cat, label] of Object.entries(CATEGORIES)) {
      const defs = BUILDINGS.filter((d) => d.category === cat);
      if (!defs.length) continue;
      const rows = defs.map((def) => {
        const unlocked = def.base || sim.unlocked.has(def.id);
        const built = sim.countBuilt(def.id);
        const cost = scaledCost(def, civ.build);
        const meta = [];
        if (def.housing) meta.push(`houses ${def.housing}`);
        if (def.storage) meta.push(`holds ${def.storage}`);
        if (def.slots) meta.push(`${def.slots} workers`);
        if (def.job && def.job.type === 'craft') {
          meta.push(`${formatCost(def.job.in)} to ${formatCost(def.job.out)}`);
        }
        if (def.job && def.job.yields) meta.push(`gathers ${Object.keys(def.job.yields).join(', ')}`);
        return el('div', { class: `cat-row${unlocked ? '' : ' locked'}` }, [
          el('div', { class: 'cat-head' }, [
            el('span', { class: 'cat-name', text: def.label }),
            el('span', { class: 'cat-count', text: built ? `x${built}` : '' }),
            unlocked
              ? button('Build', () => {
                  const placed = sim.queueBuilding(def.id);
                  if (!placed) app.setNote(`no room for a ${def.def ? def.def.label : def.label}`);
                  redraw();
                })
              : el('span', { class: 'cat-lock', text: 'locked' }),
          ]),
          el('span', { class: 'cat-cost', text: formatCost(cost) }),
          meta.length ? el('span', { class: 'cat-meta', text: meta.join(' - ') }) : null,
          def.note ? el('span', { class: 'cat-note', text: def.note }) : null,
        ]);
      });
      catalog.appendChild(el('div', { class: 'class-block' }, [el('h4', { text: label }), ...rows]));
    }
  };

  redraw();
  let since = 0;
  return {
    redraw,
    tick(dt) {
      since += dt;
      if (since < 0.6) return;
      since = 0;
      redraw();
    },
  };
}

function barOf(value) {
  return el('span', { class: 'bar' }, [
    el('span', { class: 'bar-fill build', style: { width: `${Math.round(Math.max(0, Math.min(1, value)) * 100)}%` } }),
  ]);
}
