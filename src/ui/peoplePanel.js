// People panel: the parameters that decide how settlers move, work, eat and
// age, plus a live roster of who is doing what right now.

import { boolField, clear, el, numberField, section } from './controls.js';
import { PROFESSIONS } from '../civ/people.js';
import { RES_IDS, RES } from '../civ/resources.js';

export function buildPeoplePanel(root, app) {
  const civ = app.state.civ;
  clear(root);

  const num = (group, label, key, min, max, step, hint) =>
    numberField(label, {
      value: civ[group][key],
      min,
      max,
      step,
      hint,
      onInput: (v) => {
        civ[group][key] = v;
        app.requestSave();
      },
    });

  const supplyFields = RES_IDS.filter((id) => ['wood', 'food', 'fiber', 'stone'].includes(id)).map((id) =>
    numberField(`${RES[id].label} carried in`, {
      value: civ.start.supplies[id] || 0,
      min: 0,
      max: 400,
      step: 1,
      onInput: (v) => {
        civ.start.supplies[id] = v | 0;
        app.requestSave();
      },
    }),
  );

  root.appendChild(
    section('Founding party', [
      numberField('Settlers', {
        value: civ.start.population,
        min: 1,
        max: 40,
        step: 1,
        hint: 'applied on the next restart',
        onInput: (v) => {
          civ.start.population = v | 0;
          app.requestSave();
        },
      }),
      ...supplyFields,
      boolField('Arrive with a storehouse', {
        value: civ.start.storehouse !== false,
        hint: 'off means the first thing they do is build one',
        onInput: (v) => {
          civ.start.storehouse = v;
          app.requestSave();
        },
      }),
    ]),
  );

  root.appendChild(
    section('Body and day', [
      num('people', 'Day length (s)', 'dayLength', 20, 600, 5, 'simulated seconds in one day'),
      num('people', 'Work starts', 'workStart', 0, 0.5, 0.01, 'fraction of the day'),
      num('people', 'Work ends', 'workEnd', 0.5, 1, 0.01),
      num('people', 'Walking speed', 'walkSpeed', 0.3, 10, 0.1, 'cells per second'),
      num('people', 'Path speed bonus', 'roadSpeedBonus', 0, 1.5, 0.05, 'how much a worn path helps'),
      num('people', 'Carry capacity', 'carryCapacity', 1, 80, 1, 'one load; the rest is left where it fell'),
      num('people', 'Work rate', 'workRate', 0.1, 4, 0.1, 'global multiplier on every kind of work'),
      num('people', 'Laborer share', 'laborerShare', 0, 0.9, 0.05,
        'adults kept out of workplaces to haul and build'),
    ]),
  );

  root.appendChild(
    section('Needs', [
      num('people', 'Hunger per second', 'hungerRate', 0.001, 0.1, 0.001),
      num('people', 'Eats at', 'eatAt', 0.1, 0.95, 0.05, 'hunger level that sends someone to the store'),
      num('people', 'Meal size', 'mealSize', 0.5, 10, 0.5, 'food units per meal'),
      num('people', 'Tires per second', 'tireRate', 0.001, 0.05, 0.001),
      num('people', 'Rests per second', 'sleepRate', 0.02, 1, 0.02),
      num('people', 'Starvation damage', 'starveDamage', 0.001, 0.1, 0.001),
      num('people', 'Healing per second', 'healRate', 0, 0.1, 0.002),
    ]),
  );

  root.appendChild(
    section('Life', [
      num('people', 'Years per day', 'yearsPerDay', 0.05, 3, 0.05, 'how fast people age'),
      num('people', 'Adult at (years)', 'adultAge', 4, 30, 1),
      num('people', 'Fertile until (years)', 'fertileUntil', 20, 80, 1),
      num('people', 'Births per couple per day', 'birthRate', 0, 1, 0.01,
        'thinned by food in store and by housing'),
      num('people', 'Lifespan low', 'lifespanMin', 20, 120, 1),
      num('people', 'Lifespan high', 'lifespanMax', 20, 140, 1),
      num('people', 'Sickness per day', 'sicknessRate', 0, 0.2, 0.002, 'a well nearby cuts this'),
    ]),
  );

  root.appendChild(
    section('Work rates', [
      num('work', 'Harvest rate', 'harvestRate', 0.2, 12, 0.1, 'plant mass cut per second'),
      num('work', 'Mining rate', 'mineRate', 0.1, 12, 0.1),
      num('work', 'Building rate', 'buildRate', 0.1, 12, 0.1),
      num('work', 'Crafting rate', 'craftRate', 0.1, 12, 0.1),
      num('work', 'Farming rate', 'farmRate', 0.05, 4, 0.05, 'multiplied by the fertility under the fields'),
      num('work', 'Smallest plant worth cutting', 'minHarvestMass', 0.5, 20, 0.5),
      num('work', 'Cleared ground yield', 'clearYield', 0, 1, 0.05,
        'share of a plant recovered when a building is raised over it'),
      num('work', 'Dropped load life (days)', 'pileLife', 0.2, 30, 0.2),
      num('work', 'Replanning interval (s)', 'planInterval', 0.1, 10, 0.1),
    ]),
  );

  const roster = el('div', { class: 'roster' });
  const counts = el('div', { class: 'chips' });
  root.appendChild(section('Settlers', [counts, roster]));

  const redraw = () => {
    const sim = app.civ;
    clear(counts);
    clear(roster);
    if (!sim) return;
    const stats = sim.stats();
    for (const [id, label] of Object.entries(PROFESSIONS)) {
      const n = stats.professions[id] || 0;
      if (!n) continue;
      counts.appendChild(el('span', { class: 'chip', text: `${label} ${n}` }));
    }
    const people = [...sim.people].sort((a, b) => b.age - a.age).slice(0, 40);
    for (const p of people) {
      const task = p.sleeping ? 'asleep' : p.task ? p.task.kind : 'idle';
      const carry = p.carrying ? ` ${p.carry.n} ${p.carry.res}` : '';
      roster.appendChild(
        el('div', { class: 'roster-row' }, [
          el('span', { class: 'roster-name', text: p.name }),
          el('span', { class: 'roster-job', text: `${PROFESSIONS[p.profession] || p.profession} ${Math.floor(p.age)}` }),
          el('span', { class: 'roster-task', text: `${task}${carry}` }),
          bar('hunger', 1 - p.hunger),
          bar('health', p.health),
        ]),
      );
    }
  };

  redraw();
  let since = 0;
  return {
    redraw,
    tick(dt) {
      since += dt;
      if (since < 0.5) return;
      since = 0;
      redraw();
    },
  };
}

function bar(kind, value) {
  const fill = el('span', { class: `bar-fill ${kind}`, style: { width: `${Math.round(Math.max(0, Math.min(1, value)) * 100)}%` } });
  return el('span', { class: 'bar' }, [fill]);
}
