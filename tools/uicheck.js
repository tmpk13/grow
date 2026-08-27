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
await page.click('#view-body [data-find="grid"]');
await page.click('#view-body [data-find="occupancy"]');
await page.waitForTimeout(500);
await page.screenshot({ path: `${outDir}/07-overlays.png` });

// A setting the area is built from waits for Apply rather than rebuilding the
// world under the slider. It is starred, the bar says so, and leaving the panel
// with one waiting asks first.
await page.click('.tab:text-is("World")');
await page.waitForTimeout(400);
const cols = '#panel-body [data-find="columns-x"] input[type=number]';
await page.fill(cols, '90');
await page.dispatchEvent(cols, 'input');
await page.waitForTimeout(400);
if (await page.evaluate(() => document.getElementById('restart-bar').hasAttribute('hidden'))) {
  problems.push('a setting the world is built from changed and nothing said a rebuild was waiting');
}
if (
  !(await page.$eval('#panel-body [data-find="columns-x"]', (n) => n.classList.contains('waiting')))
) {
  problems.push('the changed setting is not starred');
}
await page.screenshot({ path: `${outDir}/08a-waiting.png` });

// Leaving offers the three ways out, and staying is one of them.
await page.click('.tab:text-is("Species")');
await page.waitForTimeout(300);
if (await page.evaluate(() => document.getElementById('confirm').hasAttribute('hidden'))) {
  problems.push('leaving a panel with a rebuild waiting did not ask');
}
await page.screenshot({ path: `${outDir}/08b-confirm.png` });
await page.click('#confirm [data-do="stay"]');
await page.waitForTimeout(200);
if ((await page.getAttribute('.tab.active', 'data-tab')) !== 'world') {
  problems.push('staying put moved off the tab anyway');
}

// Discard puts the setting back the way the running world has it.
await page.click('.tab:text-is("Species")');
await page.waitForTimeout(200);
await page.click('#confirm [data-do="discard"]');
await page.waitForTimeout(500);
if ((await page.getAttribute('.tab.active', 'data-tab')) !== 'species') {
  problems.push('discarding did not move on');
}
await page.click('.tab:text-is("World")');
await page.waitForTimeout(400);
if ((await page.inputValue(cols)) === '90') problems.push('discard left the change in place');

// Apply rebuilds and clears the bar.
await page.fill(cols, '90');
await page.dispatchEvent(cols, 'input');
await page.waitForTimeout(300);
await page.click('#restart-bar [data-do="apply"]');
await page.waitForTimeout(1500);
if (!(await page.evaluate(() => document.getElementById('restart-bar').hasAttribute('hidden')))) {
  problems.push('applying left the bar up');
}
if ((await page.inputValue(cols)) !== '90') problems.push('applying did not keep the change');
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
await page.screenshot({ path: `${outDir}/08d-sprites.png` });
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
await page.screenshot({ path: `${outDir}/08e-sheet.png` });

// A motion that has taken this sheet says whether it is still what the editor
// holds, and says so again after another stroke on the sheet.
await page.click('#panel-body .btn:text-is("Standing")');
await page.waitForTimeout(500);
const takenNow = await page.textContent('#panel-body .btn:has-text("Standing")');
if (!takenNow.includes('taken')) problems.push(`taking the sheet left the button reading "${takenNow}"`);
await page.click('.tab[data-tab="draw"]');
await page.waitForTimeout(400);
await page.keyboard.press('b');
await page.mouse.move(stage.x + stage.width * 0.4, stage.y + stage.height * 0.3);
await page.mouse.down();
await page.mouse.move(stage.x + stage.width * 0.6, stage.y + stage.height * 0.6, { steps: 12 });
await page.mouse.up();
await page.waitForTimeout(500);
await page.click('.tab[data-tab="sheet"]');
await page.waitForTimeout(500);
const staleNow = await page.textContent('#panel-body .btn:has-text("Standing")');
if (!staleNow.includes('out of date')) {
  problems.push(`drawing on a taken sheet left the button reading "${staleNow}"`);
}
await page.screenshot({ path: `${outDir}/08g-sheet-moved-on.png` });

// Frames reorder by dragging, and a sheet a motion took before it was last
// drawn on says so.
const frameMarks = () =>
  page.$$eval('.frame-cell', (cells) => cells.map((c) => c.getAttribute('data-drag-at')));
await page.click('.tab[data-tab="draw"]');
await page.waitForTimeout(400);
const frameCount = await page.locator('.frame-cell').count();
if (frameCount >= 2) {
  // The frame being dragged is the one selected after the drop, so where the
  // active cell ends up is what says the strip moved.
  await page.click('.frame-cell >> nth=0');
  await page.waitForTimeout(300);
  const activeAt = () =>
    page.$$eval('.frame-cell', (cells) => cells.findIndex((c) => c.classList.contains('active')));
  if ((await activeAt()) !== 0) problems.push('clicking the first frame did not select it');

  const dragged = await page.evaluate(() => {
    const cells = [...document.querySelectorAll('.frame-cell')];
    const dt = new DataTransfer();
    const last = cells[cells.length - 1];
    cells[0].dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
    last.dispatchEvent(new DragEvent('dragover', { bubbles: true, dataTransfer: dt }));
    last.dispatchEvent(new DragEvent('drop', { bubbles: true, dataTransfer: dt }));
    return dt.getData('text/plain');
  });
  if (dragged !== '0') problems.push(`the drag carried "${dragged}" rather than the frame index`);
  await page.waitForTimeout(500);
  if ((await page.locator('.frame-cell').count()) !== frameCount) {
    problems.push('dragging a frame changed how many there are');
  }
  if ((await activeAt()) !== frameCount - 1) {
    problems.push(`the dragged frame landed at ${await activeAt()}, not the end of the strip`);
  }
  const marks = await frameMarks();
  if (marks.join(',') !== marks.map((_, i) => String(i)).join(',')) {
    problems.push(`the strip is stamped ${marks.join(',')} rather than in order`);
  }
  await page.screenshot({ path: `${outDir}/08f-frames-dragged.png` });
}

// Keys: the tools answer to the keyboard, and say which key on a desktop.
await page.click('.tab[data-tab="draw"]');
await page.waitForTimeout(400);
const toolLabel = (await page.textContent('#panel-body [data-find="pick"]')).trim();
if (toolLabel !== 'Pick (P)') problems.push(`the pick tool reads "${toolLabel}", not "Pick (P)"`);
if ((await page.locator('.group.keys').count()) === 0) problems.push('the key list is missing');
const toolNow = () =>
  page.$$eval('#panel-body .btn.active', (nodes) => (nodes[0] ? nodes[0].textContent.trim() : ''));
await page.click('#world-canvas', { position: { x: 5, y: 5 } });
for (const [key, want] of [
  ['p', 'Pick (P)'],
  ['e', 'Eraser (E)'],
  ['g', 'Fill (G)'],
  ['b', 'Pencil (B)'],
]) {
  await page.keyboard.press(key);
  await page.waitForTimeout(250);
  const now = await toolNow();
  if (now !== want) problems.push(`pressing ${key} selected "${now}", not "${want}"`);
}
// The onion switch lives in the toolbar and has to follow the key too.
const onionOn = () => page.isChecked('#onion');
const wasOnion = await onionOn();
await page.keyboard.press('o');
await page.waitForTimeout(200);
if ((await onionOn()) === wasOnion) problems.push('the onion key did not reach the toolbar switch');
await page.keyboard.press('o');
await page.waitForTimeout(200);

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

// The view menu: what the stage draws over the map is in the side panel now,
// and the label switches are one per category with walls on their own.
const pressed = (find) =>
  page.evaluate((f) => {
    const node = document.querySelector(`#view-body [data-find="${f}"]`);
    return node ? node.getAttribute('aria-pressed') : null;
  }, find);
const press = (find) => page.click(`#view-body [data-find="${find}"]`);

if (await page.evaluate(() => /Occupancy|Labels/.test(document.getElementById('stage-toolbar').textContent))) {
  problems.push('the view switches are still in the stage toolbar');
}
if ((await pressed('labels')) !== 'false') problems.push('labels should start off');
if ((await pressed('walls-and-gates')) !== 'false') problems.push('wall labels should start off');
await press('labels');
await page.waitForTimeout(150);
if ((await pressed('labels')) !== 'true') problems.push('the labels toggle did not press');
await press('all');
await page.waitForTimeout(200);
for (const kind of ['homes', 'stores', 'gathering', 'crafts', 'civic', 'walls-and-gates', 'town-names']) {
  if ((await pressed(kind)) !== 'true') problems.push(`All left ${kind} off`);
}
if ((await pressed('all')) !== 'true') problems.push('All did not read as pressed once every kind was on');
await page.waitForTimeout(600);
await page.screenshot({ path: `${outDir}/11d-labels-all.png` });
// Walls back off on their own: a ring of palisade is a hundred labels.
await press('walls-and-gates');
await page.waitForTimeout(200);
if ((await pressed('walls-and-gates')) !== 'false') problems.push('turning wall labels off did not take');
if ((await pressed('all')) !== 'false') problems.push('All still read as pressed with a kind turned off');
if ((await pressed('homes')) !== 'true') problems.push('turning walls off took the other kinds with it');
await page.screenshot({ path: `${outDir}/11e-labels-some.png` });
await press('labels');
await page.waitForTimeout(150);

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
if (
  (await page.evaluate(() => document.getElementById('move-people').getAttribute('aria-pressed'))) !==
  'true'
) {
  problems.push('the move people button does not show that it is on');
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

// Menu search: typing a setting's name and pressing enter has to land on that
// setting, wherever in the menus it lives.
const findRows = () =>
  page.$$eval('#find-results .find-hit', (rows) =>
    rows.map((r) => ({
      label: r.querySelector('.find-label').firstChild.textContent.trim(),
      path: r.querySelector('.find-path').textContent.trim(),
      meaning: !!r.querySelector('.find-why'),
    })),
  );

await page.click('.mode[data-mode="lab"]');
await page.waitForTimeout(600);
// The slash key reaches the box from anywhere that is not already a text field.
await page.click('#panel-body');
await page.keyboard.press('/');
await page.waitForTimeout(150);
if (!(await page.evaluate(() => document.activeElement.id === 'find-box'))) {
  problems.push('slash did not put the cursor in the search box');
  await page.click('#find-box');
}
await page.type('#find-box', 'wilderness warmup', { delay: 12 });
await page.waitForTimeout(250);
const found = await findRows();
if (!found.length || found[0].label !== 'Wilderness warmup (s)') {
  problems.push(`searching "wilderness warmup" ranked ${JSON.stringify(found.slice(0, 3))}`);
}
await page.screenshot({ path: `${outDir}/15-search.png` });

await page.keyboard.press('Enter');
await page.waitForTimeout(3000);
const landed = await page.evaluate(() => {
  const node = document.querySelector('#panel-body [data-find="wilderness-warmup-s"]');
  return {
    mode: document.querySelector('.mode.active')?.getAttribute('data-mode'),
    tab: document.querySelector('.tab.active')?.getAttribute('data-tab'),
    there: !!node,
    flashed: node ? node.classList.contains('found') : false,
    focused: node ? node.contains(document.activeElement) : false,
    listOpen: !document.getElementById('find-results').hasAttribute('hidden'),
  };
});
if (landed.mode !== 'settlement' || landed.tab !== 'land') {
  problems.push(`search landed on ${landed.mode}/${landed.tab}, not settlement/land`);
}
if (!landed.there) problems.push('search did not reach the control it named');
if (!landed.flashed) problems.push('search did not mark where it sent you');
if (!landed.focused) problems.push('search left the keyboard somewhere else');
if (landed.listOpen) problems.push('the results list stayed open after jumping');
await page.screenshot({ path: `${outDir}/16-search-landed.png` });

// The meaning switch, if a table was built for this index. Without one the
// switch is not offered, and search is the fuzzy one only.
const hasMeaning = await page.evaluate(
  () => !document.getElementById('find-meaning-row').hasAttribute('hidden'),
);
if (hasMeaning) {
  await page.fill('#find-box', '');
  await page.type('#find-box', 'salary', { delay: 12 });
  await page.waitForTimeout(200);
  if ((await findRows()).length) {
    problems.push('"salary" is spelled like nothing in the menus and should find nothing');
  }
  await page.click('#find-meaning');
  await page.waitForTimeout(200);
  const bymeaning = await findRows();
  if (!bymeaning.length) {
    problems.push('the meaning switch found nothing for "salary"');
  } else {
    if (!bymeaning[0].meaning) problems.push('a meaning match was not marked as one');
    console.log(`meaning: salary -> ${bymeaning[0].label} (${bymeaning[0].path})`);
  }
  await page.screenshot({ path: `${outDir}/17-search-meaning.png` });
  await page.click('#find-meaning');
} else {
  console.log('meaning: no table built, switch hidden');
}
await page.fill('#find-box', '');
await page.keyboard.press('Escape');
await page.waitForTimeout(150);
if (await page.evaluate(() => !document.getElementById('find-results').hasAttribute('hidden'))) {
  problems.push('escape did not put the results list away');
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
