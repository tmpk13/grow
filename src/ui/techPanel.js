// Technology panel: what the settlement knows, what it is working on and the
// rates behind research.

import { boolField, button, clear, el, numberField, section } from './controls.js';
import { BUILDING_BY_ID } from '../civ/buildings.js';
import { MOD_KEYS, TECHS, available, isKnown, locked, modifiers, techCost } from '../civ/tech.js';

export function buildTechPanel(root, app) {
  const civ = app.state.civ;
  clear(root);

  const num = (label, key, min, max, step, hint) =>
    numberField(label, {
      value: civ.tech[key],
      min,
      max,
      step,
      hint,
      onInput: (v) => {
        civ.tech[key] = v;
        app.requestSave();
      },
    });

  root.appendChild(
    section('Research', [
      num('Cost scale', 'costScale', 0.1, 5, 0.1, 'multiplies every tech cost'),
      num('Points per scholar per second', 'researchPerScholar', 0, 4, 0.05),
      num('Insight per person per second', 'insightPerPerson', 0, 0.1, 0.001,
        'what a settlement works out without a school'),
      num('Need bias', 'needBias', 0, 3, 0.1,
        'how strongly automatic research chases what the settlement is short of'),
      boolField('Choose research automatically', {
        value: civ.tech.autoResearch !== false,
        onInput: (v) => {
          civ.tech.autoResearch = v;
          app.requestSave();
        },
      }),
    ]),
  );

  const current = el('div', { class: 'stat-grid' });
  const modList = el('div', { class: 'chips' });
  root.appendChild(section('Progress', [current, modList]));

  const treeNode = el('div', { class: 'tech-tree' });
  root.appendChild(section('Tree', [
    el('p', { class: 'note', text: 'Pick one to make it the target; the settlement researches it next.' }),
    treeNode,
  ]));

  const redraw = () => {
    const sim = app.civ;
    clear(current);
    clear(modList);
    clear(treeNode);
    if (!sim) return;
    const mods = modifiers(sim.tech);
    const target = sim.tech.target ? TECHS.find((t) => t.id === sim.tech.target) : null;
    const rows = [
      ['Known', `${sim.tech.known.length} of ${TECHS.length}`],
      ['Points', Math.round(sim.tech.points).toString()],
      ['Spent', Math.round(sim.tech.spent).toString()],
      ['Target', target ? target.label : civ.tech.autoResearch ? 'automatic' : 'none'],
    ];
    for (const [k, v] of rows) {
      current.appendChild(el('div', { class: 'stat' }, [
        el('span', { class: 'stat-key', text: k }),
        el('span', { class: 'stat-val', text: v }),
      ]));
    }
    for (const [key, label] of Object.entries(MOD_KEYS)) {
      const value = mods[key] || 1;
      if (Math.abs(value - 1) < 0.001) continue;
      modList.appendChild(el('span', { class: 'chip', text: `${label} x${value.toFixed(2)}` }));
    }

    const groups = [
      ['Known', TECHS.filter((t) => isKnown(sim.tech, t.id)), 'known'],
      ['Available', available(sim.tech), 'open'],
      ['Locked', locked(sim.tech), 'locked'],
    ];
    for (const [label, list, cls] of groups) {
      if (!list.length) continue;
      const block = el('div', { class: 'class-block' }, [el('h4', { text: label })]);
      for (const def of list) {
        const cost = techCost(def, civ.tech);
        const unlocks = def.unlocks
          .map((id) => (BUILDING_BY_ID[id] ? BUILDING_BY_ID[id].label : id))
          .join(', ');
        const effects = Object.entries(def.effects || {})
          .map(([k, v]) => `${MOD_KEYS[k] || k} +${Math.round(v * 100)}%`)
          .join(', ');
        const isTarget = sim.tech.target === def.id;
        const row = el('div', { class: `tech-row ${cls}${isTarget ? ' target' : ''}` }, [
          el('div', { class: 'cat-head' }, [
            el('span', { class: 'cat-name', text: def.label }),
            el('span', { class: 'cat-count', text: cls === 'known' ? '' : `${cost} pts` }),
            cls === 'known'
              ? null
              : button(isTarget ? 'Target' : 'Research', () => {
                  sim.tech.target = isTarget ? null : def.id;
                  redraw();
                }),
          ]),
          def.note ? el('span', { class: 'cat-note', text: def.note }) : null,
          unlocks ? el('span', { class: 'cat-meta', text: `unlocks ${unlocks}` }) : null,
          effects ? el('span', { class: 'cat-meta', text: effects }) : null,
          cls === 'locked' && def.requires.length
            ? el('span', { class: 'cat-lock', text: `needs ${def.requires.join(', ')}` })
            : null,
          cls === 'open'
            ? el('span', { class: 'bar' }, [
                el('span', {
                  class: 'bar-fill research',
                  style: { width: `${Math.round(Math.min(1, sim.tech.points / cost) * 100)}%` },
                }),
              ])
            : null,
        ]);
        block.appendChild(row);
      }
      treeNode.appendChild(block);
    }
  };

  redraw();
  let since = 0;
  return {
    redraw,
    tick(dt) {
      since += dt;
      if (since < 0.8) return;
      since = 0;
      redraw();
    },
  };
}
