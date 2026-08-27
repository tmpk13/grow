// Loads the tool in a headless browser, clicks through both modes and every
// tab in them, and reports any console error or uncaught exception. Writes
// screenshots next to the output path given as the first argument.
//
//   bun run tools/uicheck.js /tmp/shots

import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';

const outDir = process.argv[2] || '/tmp/grow-shots';
mkdirSync(outDir, { recursive: true });

// A fresh port per run, so a server left behind by an earlier run cannot serve
// stale files to this one.
const PORT = 5200 + Math.floor(Math.random() * 300);
const server = Bun.spawn(['bun', 'run', 'serve.js'], {
  cwd: new URL('..', import.meta.url).pathname,
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

const problems = [];
page.on('console', (msg) => {
  if (msg.type() === 'error') problems.push(`console error: ${msg.text()}`);
});
page.on('pageerror', (err) => problems.push(`page error: ${err.message}`));

await page.goto(`http://localhost:${PORT}/`, { waitUntil: 'networkidle' });
await page.waitForTimeout(400);
if ((await page.locator('.tab').count()) === 0) {
  console.error('the app did not boot: no tabs rendered');
  for (const p of problems) console.error(`  ${p}`);
  await browser.close();
  server.kill();
  process.exit(1);
}

// The speed slider is logarithmic from a quarter to two hundred, so a
// multiplier has to be converted to a position on it.
const SPEED_MIN = 0.25;
const SPEED_MAX = 200;
const SPEED_STEPS = 400;
const speedPos = (x) =>
  String(Math.round((Math.log(x / SPEED_MIN) / Math.log(SPEED_MAX / SPEED_MIN)) * SPEED_STEPS));
const setSpeed = (pos) =>
  page.evaluate((value) => {
    const slider = document.querySelectorAll('.toolbar-row input[type=range]')[0];
    slider.value = value;
    slider.dispatchEvent(new Event('input', { bubbles: true }));
  }, pos);

// Run the simulation for a few seconds at speed so the stage has content.
const playBtn = page.locator('.toolbar-row .btn').first();
if ((await playBtn.textContent()).trim() === 'Play') await playBtn.click();
await page.waitForTimeout(300);
await setSpeed(speedPos(16));
await page.waitForTimeout(4000);
await page.screenshot({ path: `${outDir}/01-materials.png` });

const labTabs = ['Shading', 'Species', 'World'];
for (const tab of labTabs) {
  await page.click(`.tab:text-is("${tab}")`);
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${outDir}/0${labTabs.indexOf(tab) + 2}-${tab.toLowerCase()}.png` });
}

// Grow one specimen in the species preview.
await page.click('.tab:text-is("Species")');
await page.click('.chip:text-is("Broadleaf tree")');
await page.click('text=Grow to full');
await page.waitForTimeout(600);
await page.screenshot({ path: `${outDir}/06-preview-tree.png` });

// Paint a few pixels in the sampling grid and switch to the shared grid mode.
await page.click('.tab:text-is("Materials")');
await page.waitForTimeout(300);
const box = await page.locator('.grid-canvas').boundingBox();
await page.mouse.move(box.x + box.width * 0.3, box.y + box.height * 0.4);
await page.mouse.down();
await page.mouse.move(box.x + box.width * 0.6, box.y + box.height * 0.6, { steps: 8 });
await page.mouse.up();
await page.selectOption('.group select', 'single');
await page.waitForTimeout(800);
await page.screenshot({ path: `${outDir}/05-shared-grid.png` });

// Overlays on, then resize the world from the World panel (restarts the sim).
await page.locator('.toolbar-row input[type=checkbox]').first().check();
await page.locator('.toolbar-row input[type=checkbox]').last().check();
await page.waitForTimeout(500);
await page.screenshot({ path: `${outDir}/07-overlays.png` });

await page.click('.tab:text-is("World")');
await page.locator('.group-body .num').first().fill('90');
await page.locator('.group-body .num').first().dispatchEvent('input');
await page.waitForTimeout(1500);
await page.screenshot({ path: `${outDir}/08-resized.png` });

const stats = await page.evaluate(() => document.getElementById('statusbar').textContent);
console.log(`lab status: ${stats}`);

// The top of the slider is the highest speed the tool offers, and the readout
// beside it is what says so.
await setSpeed(String(SPEED_STEPS));
await page.waitForTimeout(300);
const topSpeed = (await page.locator('.toolbar-row .readout').first().textContent()).trim();
if (topSpeed !== `${SPEED_MAX}x`) problems.push(`top speed reads ${topSpeed}, not ${SPEED_MAX}x`);
await setSpeed(speedPos(4));

// The sprite editor is its own mode, and its surface is the stage: draw on it,
// stack a layer, step and play the frames, and send the sheet to a motion.
await page.click('.mode:text-is("Sprite editor")');
await page.waitForTimeout(700);
if ((await page.locator('.tab').allTextContents()).join() !== 'Draw,Sheet') {
  problems.push('the sprite editor did not bring its own tabs');
}
const stage = await page.locator('#world-canvas').boundingBox();
await page.mouse.move(stage.x + stage.width * 0.45, stage.y + stage.height * 0.35);
await page.mouse.down();
await page.mouse.move(stage.x + stage.width * 0.55, stage.y + stage.height * 0.55, { steps: 12 });
await page.mouse.up();
await page.waitForTimeout(900);
if (!/frame 1\//.test(await page.evaluate(() => document.getElementById('statusbar').textContent))) {
  problems.push('the sprite editor status line does not say which frame is showing');
}
await page.click('.btn:text-is("Add layer")');
await page.waitForTimeout(300);
if ((await page.locator('.layer-row').count()) < 2) problems.push('adding a layer did nothing');

// Undo: the layer comes off and goes back, from the top bar rather than the
// panel, because a step covers the whole project.
if (await page.locator('#btn-undo').isDisabled()) problems.push('undo stayed disabled after an edit');
await page.click('#btn-undo');
await page.waitForTimeout(400);
if ((await page.locator('.layer-row').count()) !== 1) problems.push('undo did not take the layer off');
await page.click('#btn-redo');
await page.waitForTimeout(400);
if ((await page.locator('.layer-row').count()) !== 2) problems.push('redo did not put the layer back');

await page.click('.btn:text-is("Duplicate frame")');
await page.waitForTimeout(300);
if ((await page.locator('.frame-cell').count()) < 2) problems.push('duplicating a frame did nothing');
// Nudging the art, and stepping that back too.
await page.click('.btn:text-is("Nudge right")');
await page.waitForTimeout(300);
await page.click('#btn-undo');
await page.waitForTimeout(300);
await page.click('.btn:text-is("Wheel")');
await page.waitForTimeout(300);
if ((await page.locator('.wheel-canvas').count()) === 0) problems.push('the color wheel did not open');
const wheel = await page.locator('.wheel-canvas').boundingBox();
await page.mouse.click(wheel.x + wheel.width * 0.65, wheel.y + wheel.height * 0.5);

// The transport is on the stage toolbar, beside the camera. Duplicating a
// frame lands on the duplicate, so the sheet is already on its last frame and
// a step forward from here is the one that wraps.
const readout = async () => (await page.locator('#frame-readout').textContent()).trim();
if ((await readout()) !== '2/2') problems.push('duplicating a frame did not select it');
await page.click('.toolbar-row .btn:text-is("Next")');
await page.waitForTimeout(300);
if ((await readout()) !== '1/2') problems.push('stepping past the last frame did not wrap');
await page.click('.toolbar-row .btn:text-is("Prev")');
await page.waitForTimeout(300);
if ((await readout()) !== '2/2') problems.push('stepping back before the first frame did not wrap');
await page.click('.toolbar-row .btn:text-is("Play")');
await page.waitForTimeout(900);
await page.screenshot({ path: `${outDir}/08b-sprites.png` });
await page.click('.toolbar-row .btn:text-is("Pause")');

// A plain panel field is a step too, which is the point of the undo rework.
await page.click('.tab:text-is("Sheet")');
await page.waitForTimeout(400);
const rate = page.locator('.field', { has: page.locator('.field-label:text-is("Rate")') }).locator('.num');
const rateBefore = await rate.inputValue();
await rate.fill('11');
await rate.dispatchEvent('input');
await page.waitForTimeout(400);
await page.click('#btn-undo');
await page.waitForTimeout(500);
const rateAfter = await page
  .locator('.field', { has: page.locator('.field-label:text-is("Rate")') })
  .locator('.num')
  .inputValue();
if (rateAfter !== rateBefore) {
  problems.push(`undo left the rate at ${rateAfter}, not ${rateBefore}`);
}
await page.click('.group:has-text("Use as settler art") .btn:text-is("Walking")');
await page.waitForTimeout(400);
await page.screenshot({ path: `${outDir}/08c-sheet.png` });

// Settlement mode: founding the settlement runs the wilderness warmup, which
// takes a moment, so give it room before touching the panels.
await page.click('.mode:text-is("Settlement")');
await page.waitForTimeout(9000);
await page.screenshot({ path: `${outDir}/09-settlement.png` });
for (const tab of ['People', 'Build', 'Economy', 'Tech']) {
  await page.click(`.tab:text-is("${tab}")`);
  await page.waitForTimeout(900);
  await page.screenshot({ path: `${outDir}/10-${tab.toLowerCase()}.png` });
}
// The register: open a settler's record, resort the list, include the dead.
// This is the one panel that rebuilds interactive rows on a timer, so it is
// also the one that would leak a listener per row if the scopes were wrong.
await page.click('.tab:text-is("People")');
await page.waitForTimeout(600);
// Paused first: the register rebuilds twice a second and its rows reorder as
// settlers change what they are doing, so a click on a live list races the
// redraw that replaces the row under it.
const pause = async () => {
  if ((await page.locator('#btn-play').textContent()).trim() === 'Pause') {
    await page.click('#btn-play');
  }
};
const resume = async () => {
  if ((await page.locator('#btn-play').textContent()).trim() === 'Play') {
    await page.click('#btn-play');
  }
};
await pause();
await page.waitForTimeout(300);
await page.locator('.roster-name.link').first().click();
await page.waitForTimeout(700);
if ((await page.locator('.person-card .stat').count()) === 0) {
  problems.push('clicking a settler did not open their record');
}
await page.screenshot({ path: `${outDir}/10b-person.png` });
await page.click('.chip:text-is("Coin")');
await page.click('.chip:text-is("Include the dead")');
await page.waitForTimeout(700);
await page.screenshot({ path: `${outDir}/10c-register.png` });
await resume();

await page.click('.tab:text-is("Build")');
await page.locator('.cat-row:not(.locked) .btn:text-is("Build")').first().click();
await setSpeed(speedPos(16));
await page.waitForTimeout(6000);
await page.screenshot({ path: `${outDir}/11-settlement-run.png` });
const civStats = await page.evaluate(() => document.getElementById('statusbar').textContent);
console.log(`settlement status: ${civStats}`);
if (!/people \d+/.test(civStats)) problems.push('settlement status line has no population');

// Moving people: with the switch on, a press on a settler picks them up and
// the pointer carries them until it is let go. Where the settlers are on
// screen is not knowable from out here, so the stage is swept from inside the
// page until one comes up in hand.
await pause();
await page.click('#move-people');
await page.waitForTimeout(200);
if (!(await page.evaluate(() => document.body.classList.contains('moving-people')))) {
  problems.push('the move people switch did not change what a press on the stage does');
}
const sweepStage = () => page.evaluate(() => {
  const canvas = document.getElementById('world-canvas');
  const r = canvas.getBoundingClientRect();
  const note = () => (document.getElementById('save-note').textContent || '').trim();
  const send = (type, x, y) =>
    canvas.dispatchEvent(
      new PointerEvent(type, {
        pointerId: 1,
        clientX: x,
        clientY: y,
        bubbles: true,
        button: 0,
        buttons: type === 'pointerup' ? 0 : 1,
      }),
    );
  for (let y = r.top + 4; y < r.bottom - 4; y += 6) {
    for (let x = r.left + 4; x < r.right - 4; x += 6) {
      send('pointerdown', x, y);
      if (note().startsWith('holding')) return { x, y, who: note() };
      send('pointerup', x, y);
    }
  }
  return null;
});
// Settlers indoors are not on the map to be picked up, so a town that has
// gone to bed has nobody out there at all. The clock is run on between
// sweeps until somebody is up.
let sweep = null;
for (let attempt = 0; attempt < 10 && !sweep; attempt += 1) {
  if (attempt > 0) {
    await resume();
    await page.waitForTimeout(2500);
    await pause();
    await page.waitForTimeout(200);
  }
  sweep = await sweepStage();
}
if (!sweep) {
  problems.push('no settler could be picked up anywhere on the stage');
} else {
  console.log(`picked up: ${sweep.who}`);
  if (!(await page.evaluate(() => document.body.classList.contains('holding')))) {
    problems.push('holding a settler did not show in the pointer');
  }
  await page.screenshot({ path: `${outDir}/11b-holding.png` });
  const put = await page.evaluate(({ x, y }) => {
    const canvas = document.getElementById('world-canvas');
    const send = (type, cx, cy) =>
      canvas.dispatchEvent(
        new PointerEvent(type, {
          pointerId: 1,
          clientX: cx,
          clientY: cy,
          bubbles: true,
          button: 0,
          buttons: type === 'pointerup' ? 0 : 1,
        }),
      );
    send('pointermove', x + 40, y + 24);
    send('pointerup', x + 40, y + 24);
    return (document.getElementById('save-note').textContent || '').trim();
  }, sweep);
  if (!put.startsWith('put ')) problems.push(`putting a settler down said "${put}"`);
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${outDir}/11c-put-down.png` });
}
await page.click('#move-people');
await page.waitForTimeout(200);
if (await page.evaluate(() => document.body.classList.contains('moving-people'))) {
  problems.push('turning the move people switch off left the stage picking settlers up');
}
await resume();

// Back to the lab and in again: both sims have to survive the switch.
await page.click('.mode:text-is("Plant lab")');
await page.waitForTimeout(1200);
await page.click('.mode:text-is("Settlement")');
await page.waitForTimeout(2000);
await page.screenshot({ path: `${outDir}/12-settlement-return.png` });

// The menu folds away and comes back, and the map takes the room either way.
await page.click('#btn-panel');
await page.waitForTimeout(500);
if (await page.locator('.panel').isVisible()) problems.push('hiding the menu left it on screen');
await page.screenshot({ path: `${outDir}/13-menu-hidden.png` });
await page.click('#btn-panel');
await page.waitForTimeout(400);
if (!(await page.locator('.panel').isVisible())) problems.push('showing the menu did not bring it back');

// Fullscreen: every piece of chrome goes and the world keeps the window.
await page.click('#btn-full');
await page.waitForTimeout(900);
if (await page.locator('.topbar').isVisible()) problems.push('fullscreen left the top bar on screen');
if (await page.locator('.statusbar').isVisible()) problems.push('fullscreen left the status line on screen');
if (!(await page.locator('.stage-escape').isVisible())) problems.push('fullscreen left no way out');
await page.screenshot({ path: `${outDir}/14-fullscreen.png` });
// Escape is the browser's own, and all the page sees of it is the event. A
// browser that refused the request has none to leave, so the button stands in.
const leftBy = await page.evaluate(() => {
  if (!document.fullscreenElement) return 'button';
  document.exitFullscreen();
  return 'event';
});
if (leftBy === 'button') await page.click('#btn-leave-full');
await page.waitForTimeout(900);
if (!(await page.locator('.topbar').isVisible())) {
  problems.push(`leaving fullscreen by ${leftBy} did not bring the chrome back`);
}
if ((await page.locator('#btn-full').textContent()).trim() !== 'Fullscreen') {
  problems.push('the fullscreen button kept its leaving label');
}

// Text scale reaches the whole page through the root font size.
const scaled = await page.evaluate(() => {
  const input = document.getElementById('ui-scale');
  const before = parseFloat(getComputedStyle(document.documentElement).fontSize);
  input.value = '1.5';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  const after = parseFloat(getComputedStyle(document.documentElement).fontSize);
  input.value = '1';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  return after / before;
});
if (scaled < 1.4) problems.push(`text scale moved the root size by ${scaled.toFixed(2)}, not 1.5`);

await browser.close();
server.kill();

if (problems.length) {
  console.error(`problems (${problems.length}):`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`no console errors. screenshots in ${outDir}`);
