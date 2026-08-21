// Economy panel: the store, the prices that come out of it, the treasury and
// the parameters behind all three.

import { boolField, clear, el, numberField, section } from './controls.js';
import { RES, RES_IDS } from '../civ/resources.js';
import { netWorth, priceOf, stockTargets } from '../civ/economy.js';

export function buildEconomyPanel(root, app) {
  const civ = app.state.civ;
  clear(root);

  const num = (label, key, min, max, step, hint) =>
    numberField(label, {
      value: civ.economy[key],
      min,
      max,
      step,
      hint,
      onInput: (v) => {
        civ.economy[key] = v;
        app.requestSave();
      },
    });

  const table = el('div', { class: 'stock-table' });
  const summary = el('div', { class: 'stat-grid' });
  const plotWrap = el('div', { class: 'plot-wrap' });
  const plot = el('canvas', { class: 'plot-canvas' });
  plotWrap.appendChild(plot);
  const log = el('div', { class: 'event-log' });

  root.appendChild(section('Store', [summary, table]));
  root.appendChild(section('History', [plotWrap, el('p', { class: 'note', text:
    'One sample per day: population, food in store and coin in the treasury.' })]));

  root.appendChild(
    section('Prices and money', [
      el('p', { class: 'note', text:
        'Nothing sets a price directly. Each resource has a target stock that grows with the ' +
        'population, and its price is the base price scaled by how far the store is from it.' }),
      num('Stock target per person', 'stockPerPerson', 0.5, 20, 0.5),
      num('Raw weight', 'rawWeight', 0.1, 4, 0.1),
      num('Made weight', 'madeWeight', 0.1, 4, 0.1),
      num('Price elasticity', 'elasticity', 0.1, 2.5, 0.05, 'how hard scarcity moves a price'),
      num('Price smoothing', 'priceSmoothing', 0.01, 2, 0.01),
      num('Hoard limit', 'hoardLimit', 1, 8, 0.25,
        'stock above this multiple of the target is left on the ground'),
      num('Starting treasury', 'startCoin', 0, 2000, 10),
      num('Wage per work second', 'wage', 0, 5, 0.05, 'paid only once a market stands'),
      boolField('Pay wages', {
        value: civ.economy.paysWages !== false,
        onInput: (v) => {
          civ.economy.paysWages = v;
          app.requestSave();
        },
      }),
    ]),
  );

  root.appendChild(
    section('Caravans', [
      el('p', { class: 'note', text:
        'A market brings caravans. They buy whatever the settlement has too much of and sell it ' +
        'what it is short of, both at the settlement price shifted by the margin.' }),
      num('Days between visits', 'tradeInterval', 10, 600, 5, 'in simulated seconds'),
      num('Units per visit', 'tradeVolume', 1, 400, 1),
      num('Trade margin', 'tradeMargin', 0, 0.9, 0.05),
      num('Caravan purse', 'caravanCoin', 0, 5000, 20),
      el('h4', { class: 'sub-head', text: 'Recent events' }),
      log,
    ]),
  );

  const redraw = () => {
    const sim = app.civ;
    clear(table);
    clear(summary);
    clear(log);
    if (!sim) return;
    const targets = stockTargets(civ.economy, sim.people.length);

    table.appendChild(headerRow(['Resource', 'Stock', 'Target', 'Price', 'In/day', 'Out/day']));
    for (const id of RES_IDS) {
      const stock = sim.stock[id] || 0;
      table.appendChild(
        el('div', { class: 'stock-row' }, [
          el('span', { class: 'swatch-dot', style: { background: RES[id].color } }),
          el('span', { class: 'stock-name', text: RES[id].label }),
          el('span', { class: 'stock-num', text: Math.round(stock).toString() }),
          el('span', { class: 'stock-num dim', text: Math.round(targets[id]).toString() }),
          el('span', { class: 'stock-num', text: priceOf(sim.econ, id).toFixed(1) }),
          el('span', { class: 'stock-num up', text: sim.econ.rateIn[id].toFixed(0) }),
          el('span', { class: 'stock-num down', text: sim.econ.rateOut[id].toFixed(0) }),
        ]),
      );
    }

    const stats = sim.stats();
    const rows = [
      ['Treasury', `${Math.round(sim.econ.coin)} coin`],
      ['Net worth', `${Math.round(netWorth(sim.econ, sim.stock))} coin`],
      ['Storage used', `${Math.round(stats.bulk)} / ${stats.storage}`],
      ['Caravans', String(sim.econ.trades)],
      ['Trade balance', `${Math.round(sim.econ.tradeBalance)} coin`],
      ['Unpaid wages', Math.round(sim.econ.unpaidWages).toString()],
      ['Loads on the ground', String(sim.piles.length)],
    ];
    for (const [k, v] of rows) {
      summary.appendChild(el('div', { class: 'stat' }, [
        el('span', { class: 'stat-key', text: k }),
        el('span', { class: 'stat-val', text: v }),
      ]));
    }

    for (const e of sim.econ.events.slice(-8).reverse()) {
      log.appendChild(el('div', { class: 'event', text: `day ${e.day}  ${e.text}` }));
    }
    drawHistory(plot, sim);
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

function headerRow(labels) {
  return el('div', { class: 'stock-row head' }, [
    el('span', { class: 'swatch-dot ghost' }),
    ...labels.map((l, i) => el('span', { class: i === 0 ? 'stock-name' : 'stock-num', text: l })),
  ]);
}

// Three series on one plot, each scaled to its own maximum, because the point
// is the shape of the run rather than the absolute numbers.
function drawHistory(canvas, sim) {
  const ctx = canvas.getContext('2d');
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(rect.width * dpr));
  const h = Math.max(1, Math.round(rect.height * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, rect.width, rect.height);
  ctx.fillStyle = '#0b0f14';
  ctx.fillRect(0, 0, rect.width, rect.height);
  const history = sim.econ.history;
  if (history.length < 2) return;

  const series = [
    { key: 'pop', color: '#7fd1a0' },
    { key: 'food', color: '#9fd06a' },
    { key: 'coin', color: '#ffc978' },
    { key: 'buildings', color: '#7fb4ff' },
  ];
  for (const s of series) {
    let max = 1;
    for (const sample of history) max = Math.max(max, sample[s.key] || 0);
    ctx.strokeStyle = s.color;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    history.forEach((sample, i) => {
      const x = (i / (history.length - 1)) * rect.width;
      const y = rect.height - ((sample[s.key] || 0) / max) * (rect.height - 4) - 2;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();
  }
  ctx.fillStyle = 'rgba(141, 155, 176, 0.9)';
  ctx.font = '10px ui-monospace, monospace';
  ctx.fillText(`day ${history[0].day} - ${history[history.length - 1].day}`, 4, 11);
}
