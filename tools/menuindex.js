// Walks the built page in a headless browser, visiting every mode and every
// tab in it, and writes what the menus actually contain to
// assets/menu-index.json. That file is baked into the next build, so menu
// search can only ever offer controls the page really has.
//
// Run it after changing a panel, then build again:
//
//   bun run build && bun run index:menu && bun run build
//
// With --check it writes nothing and exits non-zero if the committed index has
// drifted from the page.

import { chromium } from 'playwright-core';
import { readFileSync, writeFileSync } from 'node:fs';

const root = new URL('..', import.meta.url).pathname;
const outPath = `${root}assets/menu-index.json`;
const checkOnly = process.argv.includes('--check');

const PORT = 5600 + Math.floor(Math.random() * 300);
const server = Bun.spawn(['bun', 'run', 'serve.js'], {
  cwd: root,
  env: { ...process.env, PORT: String(PORT) },
  stdout: 'pipe',
  stderr: 'pipe',
});
await Bun.sleep(500);

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM_PATH,
  args: ['--no-sandbox'],
});
const page = await browser.newPage({ viewport: { width: 1500, height: 950 } });

const fail = async (msg) => {
  console.error(msg);
  await browser.close();
  server.kill();
  process.exit(1);
};

await page.goto(`http://localhost:${PORT}/`, { waitUntil: 'networkidle' });
await page.waitForTimeout(400);
if ((await page.locator('.tab').count()) === 0) await fail('the app did not boot: no tabs rendered');

// The chrome is the same on every screen, so it is read once and carries no
// mode. Everything here is addressed by id because none of it is generated.
const chrome = [
  ['#btn-undo', 'Undo', 'step back through changes'],
  ['#btn-redo', 'Redo', 'step forward again'],
  ['#btn-panel', 'Hide menu', 'fold the side menu away and give its width to the world'],
  ['#btn-fold-groups', 'Fold all', 'fold every section shut, or open them all again'],
  ['#btn-full', 'Fullscreen', 'show just the world, no menus'],
  ['#btn-new', 'New', 'start an empty project'],
  ['#btn-export', 'Export', 'save the project to a file'],
  ['#file-import', 'Import', 'load a project from a file'],
  ['#btn-reset', 'Reset all', 'throw everything away and start over'],
  ['#ui-scale', 'Text size', 'how large every label and control is drawn'],
  ['#find-meaning', 'Meaning', 'match on what a menu means, not only how it is spelled'],
].map(([anchor, label, hint]) => ({
  mode: '',
  mode_label: '',
  tab: '',
  tab_label: '',
  group: 'Everywhere',
  label,
  hint,
  anchor,
  kind: 'chrome',
}));

// Everything a panel drew, read straight out of the page it drew it into.
const harvest = (mode, modeLabel, tab, tabLabel) =>
  page.evaluate(
    ([mode, modeLabel, tab, tabLabel]) => {
      const text = (node) => (node ? node.textContent.trim().replace(/\s+/g, ' ') : '');
      const out = [];
      const seen = new Set();
      for (const node of document.querySelectorAll('#panel-body [data-find]')) {
        const anchor = node.getAttribute('data-find');
        const label = text(node.querySelector('.field-label')) || text(node);
        if (!anchor || !label || seen.has(anchor)) continue;
        seen.add(anchor);
        const group = node.closest('[data-group]');
        out.push({
          mode,
          mode_label: modeLabel,
          tab,
          tab_label: tabLabel,
          group: group ? group.getAttribute('data-group') : '',
          label,
          hint: text(node.querySelector('.field-hint')),
          anchor,
          kind: node.tagName === 'BUTTON' ? 'button' : 'field',
        });
      }
      return out;
    },
    [mode, modeLabel, tab, tabLabel],
  );

const modes = await page.$$eval('#modes .mode', (nodes) =>
  nodes.map((n) => ({ id: n.getAttribute('data-mode'), label: n.textContent.trim() })),
);

const entries = [];
for (const { id: mode, label: modeLabel } of modes) {
  if (!mode) await fail(`the mode button "${modeLabel}" carries no data-mode`);
  await page.click(`.mode[data-mode="${mode}"]`);
  // The settlement grows its wilderness on the first visit, which blocks the
  // thread; nothing is in the panel until it is done.
  await page.waitForTimeout(mode === 'settlement' ? 3000 : 400);

  // The view menu sits beside the tabs and belongs to the mode, not to any one
  // of them, so it is read once per mode.
  entries.push(
    ...(await page.evaluate(
      ([mode, modeLabel]) =>
        [...document.querySelectorAll('#view-body [data-find]')].map((node) => ({
          mode,
          mode_label: modeLabel,
          tab: '',
          tab_label: '',
          group: 'View',
          label: node.textContent.trim(),
          hint: node.getAttribute('title') || '',
          anchor: node.getAttribute('data-find'),
          kind: 'view',
        })),
      [mode, modeLabel],
    )),
  );

  const tabs = await page.$$eval('#tabs .tab', (nodes) =>
    nodes.map((n) => ({ id: n.getAttribute('data-tab'), label: n.textContent.trim() })),
  );
  for (const { id: tab, label: tabLabel } of tabs) {
    if (!tab) await fail(`the tab "${tabLabel}" carries no data-tab`);
    await page.click(`.tab[data-tab="${tab}"]`);
    await page.waitForTimeout(500);
    entries.push({
      mode,
      mode_label: modeLabel,
      tab,
      tab_label: tabLabel,
      group: '',
      label: tabLabel,
      hint: `the ${tabLabel.toLowerCase()} tab of ${modeLabel.toLowerCase()}`,
      anchor: '',
      kind: 'tab',
    });
    entries.push(...(await harvest(mode, modeLabel, tab, tabLabel)));
  }
}

entries.push(...chrome);

await browser.close();
server.kill();

if (entries.length < 50) {
  console.error(`only ${entries.length} entries: the panels cannot all have been read`);
  process.exit(1);
}

// Empty fields are the default on the Rust side, so leaving them out keeps
// the file readable without changing what it says.
const drop = (_key, value) => (value === '' ? undefined : value);
const json = `${JSON.stringify(entries, drop, 2)}\n`;
if (checkOnly) {
  if (readFileSync(outPath, 'utf8') !== json) {
    console.error(`assets/menu-index.json is stale: ${entries.length} entries in the page`);
    console.error('run: bun run index:menu && bun run build');
    process.exit(1);
  }
  console.log(`menu index is current: ${entries.length} entries`);
} else {
  const changed = readFileSync(outPath, 'utf8') !== json;
  writeFileSync(outPath, json);
  console.log(`wrote ${entries.length} entries to assets/menu-index.json`);
  // The meaning table points at entries by position, so it is only good for
  // the index it was built against and goes quietly unused otherwise.
  const terms = readFileSync(`${root}assets/menu-terms.json`, 'utf8');
  if (changed && terms.includes('"stamp"')) {
    console.log('the menus moved: rebuild the meaning table with `bun run index:terms`');
  }
}
