// Loads the tool in a headless browser, clicks through every mode and every
// tab in them, and reports any console error or uncaught exception. Writes
// screenshots next to the output path given as the first argument.
//
//   bun run tools/uicheck.js /tmp/shots

import { chromium } from 'playwright-core';
import { mkdirSync, readFileSync } from 'node:fs';

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
const page = await browser.newPage({ viewport: { width: 1500, height: 950 }, acceptDownloads: true });

const problems = [];
page.on('console', (msg) => {
  if (msg.type() === 'error') problems.push(`console error: ${msg.text()}`);
});
page.on('pageerror', (err) => problems.push(`page error: ${err.message}`));

// Sections of a panel arrive folded, and nearly every check below reaches
// into one. Pressing Unfold all after each of them would be a press after
// everything that rebuilds a panel, which is most of what this file does, so
// the page holds them open for itself instead - the state somebody who had
// pressed it once would be working in. The switch is in local storage so it
// survives the reload at the end, and it starts off, which is how the check
// just below can see a panel arrive folded at all.
await page.addInitScript(() => {
  // A timer rather than a MutationObserver: this runs before the document
  // exists, so there is nothing to observe yet, and a panel is rebuilt often
  // enough that a tenth of a second is not worth being clever about.
  setInterval(() => {
    let on = false;
    try {
      on = localStorage.getItem('uicheck.folds') === 'open';
    } catch {
      return;
    }
    if (!on) return;
    for (const group of document.querySelectorAll('#panel-body details.group')) {
      if (!group.hasAttribute('open')) group.setAttribute('open', 'open');
    }
  }, 100);
});
const holdOpen = (on) =>
  page.evaluate((on) => localStorage.setItem('uicheck.folds', on ? 'open' : 'leave'), on);

await page.goto(`http://localhost:${PORT}/`, { waitUntil: 'networkidle' });
await page.waitForTimeout(400);
if ((await page.locator('.tab').count()) === 0) {
  console.error('the app did not boot: no tabs rendered');
  for (const p of problems) console.error(`  ${p}`);
  await browser.close();
  server.kill();
  process.exit(1);
}

// The settlement is the mode the tool opens in, and founding it runs the
// wilderness warmup, which blocks for a moment.
await page.waitForTimeout(9000);
await page.screenshot({ path: `${outDir}/00-opening.png` });
if ((await page.getAttribute('.mode.active', 'data-mode')) !== 'settlement') {
  problems.push('the tool did not open on the settlement');
}
if (!/day \d+/.test(await page.evaluate(() => document.getElementById('statusbar').textContent))) {
  problems.push('the settlement it opened on is not running');
}
// A panel arrives folded: a tab is a list of headings rather than a wall of
// controls. Checked here, before the page is asked to hold them open.
if ((await page.locator('#panel-body details.group[open]').count()) !== 0) {
  problems.push('a panel did not arrive folded');
}
await holdOpen(true);
await page.waitForTimeout(500);
if ((await page.locator('#panel-body details.group[open]').count()) === 0) {
  console.error('the run could not hold the menu sections open');
  process.exit(1);
}
// Everything below works the lab over first, so go there by hand.
await page.click('.mode:text-is("Plant lab")');
await page.waitForTimeout(800);

// The view menu is a dropdown in the top bar: anything inside it has to be
// dropped open before it can be pressed, and a press anywhere else folds it
// shut again.
const openView = async () => {
  if ((await page.getAttribute('#view-menu', 'open')) === null) {
    await page.click('#view-menu > summary');
    await page.waitForTimeout(150);
  }
};
// For a person any press elsewhere folds the dropdown away, but the test
// driver refuses to press what the open body is floating over, so every block
// that opens it folds it shut behind itself.
const closeView = async () => {
  if ((await page.getAttribute('#view-menu', 'open')) !== null) {
    await page.click('#view-menu > summary');
    await page.waitForTimeout(150);
  }
};

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

// The sections of a panel fold. This is the one block the page is not holding
// open for, so it starts from open and works both ways: the button folds them
// all and then offers the way back, a head folds its own section, and a fold
// survives the panel being rebuilt.
await holdOpen(false);
await page.waitForTimeout(200);
const openGroups = () => page.locator('#panel-body details.group[open]').count();
const allGroups = () => page.locator('#panel-body details.group').count();
if ((await openGroups()) !== (await allGroups())) {
  problems.push('Unfold all left sections folded');
}
await page.click('#btn-fold-groups');
await page.waitForTimeout(300);
if ((await openGroups()) !== 0) problems.push('Fold all left sections open');
if ((await page.locator('#btn-fold-groups').textContent()).trim() !== 'Unfold all') {
  problems.push('the fold-all button does not offer the way back');
}
await page.click('#btn-fold-groups');
await page.waitForTimeout(300);
if ((await openGroups()) !== (await allGroups())) {
  problems.push('Unfold all left sections folded');
}
const firstHead = page.locator('#panel-body details.group summary').first();
await firstHead.click();
await page.waitForTimeout(200);
await page.click('.tab:text-is("Species")');
await page.waitForTimeout(300);
await page.click('.tab:text-is("Materials")');
await page.waitForTimeout(300);
const firstGroup = page.locator('#panel-body details.group').first();
if ((await firstGroup.getAttribute('open')) !== null) {
  problems.push('a folded section sprang open when its panel was rebuilt');
}
await firstGroup.locator('summary').click();
await page.waitForTimeout(200);
await holdOpen(true);
await page.waitForTimeout(300);

// Overlays on, then resize the world from the World panel (restarts the sim).
// The switches live in the top bar dropdown.
await openView();
await page.click('#view-body [data-find="grid"]');
await page.click('#view-body [data-find="occupancy"]');
await page.waitForTimeout(500);
await page.screenshot({ path: `${outDir}/07-overlays.png` });
await closeView();

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
// The editor draws none of the view menu's overlays, so the dropdown is gone.
if (await page.locator('#view-menu').isVisible()) {
  problems.push('the view dropdown is showing in the sprite editor');
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
await page.click('.group:has-text("Use as person art") .btn:text-is("Walking")');
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

// The canvas goes well past the sixty four pixels it was once capped at: a
// large building at a wide cell needs the room, and a picture is drawn at the
// size its own pixels say. The panel says what the frame is worth in cells.
const widthField = '#panel-body [data-find="frame-width"] input.num';
const wasWide = await page.inputValue(widthField);
await page.fill(widthField, '160');
await page.waitForTimeout(700);
const nowWide = await page.inputValue(widthField);
if (nowWide !== '160') {
  problems.push(`the frame would not go to 160 px wide: it reads ${nowWide}`);
}
const cellNote = await page.$$eval('#panel-body .note', (n) =>
  n.map((x) => x.textContent).find((t) => t.includes('pixels to a cell')),
);
if (!/stands [\d.]+ by [\d.]+ cells/.test(cellNote ?? '')) {
  problems.push(`the sheet does not say what it is worth in cells: "${cellNote}"`);
}
await page.fill(widthField, wasWide);
await page.waitForTimeout(700);

// Downloads: one frame as a PNG, and the ticked sheets as a zip.
const grab = async (selector) => {
  const [download] = await Promise.all([
    page.waitForEvent('download', { timeout: 8000 }),
    page.click(selector),
  ]);
  return { name: download.suggestedFilename(), bytes: readFileSync(await download.path()) };
};
const frame = await grab('#panel-body .btn:text-is("Download this frame")');
if (frame.bytes.subarray(1, 4).toString() !== 'PNG') {
  problems.push(`downloading a frame gave ${frame.name}, which is not a png`);
}
const zip = await grab('#panel-body .btn:text-is("Download zip")');
if (zip.bytes.subarray(0, 2).toString() !== 'PK') {
  problems.push(`downloading a zip gave ${zip.name}, which is not an archive`);
}
// The end record says how many files are in it, and where the directory is.
const end = zip.bytes.length - 22;
if (zip.bytes.readUInt32LE(end) !== 0x06054b50) {
  problems.push('the zip has no end record where one should be');
} else {
  const count = zip.bytes.readUInt16LE(end + 10);
  const at = zip.bytes.readUInt32LE(end + 16);
  const size = zip.bytes.readUInt32LE(end + 12);
  if (count < 1) problems.push('the zip holds no files');
  if (at + size !== end) problems.push('the zip directory does not run up to its end record');
}
// With nothing ticked it says so rather than handing out an empty archive.
const chips = await page.locator('#panel-body .group:has-text("Download") .chips .btn').count();
for (let i = 0; i < chips; i += 1) {
  await page.click(`#panel-body .group:has-text("Download") .chips .btn >> nth=${i}`);
}
await page.click('#panel-body .btn:text-is("Download zip")');
await page.waitForTimeout(400);
if (!(await page.textContent('#save-note')).includes('nothing ticked')) {
  problems.push('a zip with no sheets ticked did not say so');
}
for (let i = 0; i < chips; i += 1) {
  await page.click(`#panel-body .group:has-text("Download") .chips .btn >> nth=${i}`);
}

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

// The marquee: drag one out on the stage, nudge what is inside it, and drop it.
await page.click('.tab[data-tab="draw"]');
await page.waitForTimeout(400);
await page.keyboard.press('m');
await page.waitForTimeout(300);
await page.mouse.move(stage.x + stage.width * 0.42, stage.y + stage.height * 0.32);
await page.mouse.down();
await page.mouse.move(stage.x + stage.width * 0.58, stage.y + stage.height * 0.58, { steps: 10 });
await page.mouse.up();
await page.waitForTimeout(500);
if ((await page.locator('.marquee-row').count()) === 0) {
  problems.push('dragging with the marquee tool selected nothing');
}
await page.screenshot({ path: `${outDir}/08h-marquee.png` });
const said = await page.textContent('.marquee-row .field-hint');
if (!/^\d+ by \d+ at \d+,\d+/.test(said.trim())) {
  problems.push(`the selection reads "${said.trim()}"`);
}
// Nudging with a selection moves what is inside it and leaves the rest.
await page.click('.btn:text-is("Nudge right")');
await page.waitForTimeout(400);
if (await page.locator('#btn-undo').isDisabled()) {
  problems.push('nudging a selection recorded nothing to undo');
}
await page.click('#btn-undo');
await page.waitForTimeout(300);
// Escape drops it, and the row goes with it.
await page.click('#world-canvas', { position: { x: 5, y: 5 } });
await page.keyboard.press('Escape');
await page.waitForTimeout(400);
if ((await page.locator('.marquee-row').count()) !== 0) {
  problems.push('escape did not drop the selection');
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
const onionOn = async () =>
  (await page.getAttribute('#onion', 'aria-pressed')) === 'true';
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
for (const tab of ['People', 'Build', 'Economy', 'Tech', 'Experimental']) {
  await page.click(`.tab:text-is("${tab}")`);
  await page.waitForTimeout(900);
  await page.screenshot({ path: `${outDir}/10-${tab.toLowerCase()}.png` });
}
// Experiments are off, and nothing under the switch is on the page until it is
// turned on.
await page.click('.tab:text-is("Experimental")');
await page.waitForTimeout(600);
if ((await page.locator('#panel-body [data-find="send-them-up"]').count()) !== 0) {
  problems.push('an experiment was on the page with the experiments switch off');
}
await page.click('#panel-body [data-find="try-the-unfinished-things"] .btn');
await page.waitForTimeout(700);
if ((await page.locator('#panel-body [data-find="send-them-up"]').count()) === 0) {
  problems.push('turning the experiments switch on brought nothing with it');
}
await page.screenshot({ path: `${outDir}/10h-experimental.png` });
await page.click('#panel-body [data-find="try-the-unfinished-things"] .btn');
await page.waitForTimeout(400);

// The register: open a person's record, resort the list, include the dead.
// This is the one panel that rebuilds interactive rows on a timer, so it is
// also the one that would leak a listener per row if the scopes were wrong.
await page.click('.tab:text-is("People")');
await page.waitForTimeout(600);
// Paused first: the register rebuilds twice a second and its rows reorder as
// people change what they are doing, so a click on a live list races the
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
  problems.push('clicking a person did not open their record');
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

// Pictures for made things: every catalog entry has a slot, and a sheet sent to
// one fills it and turns pictures on. That the picture then reaches the map is
// checked in the simulation tests, where a sprite can be read rather than
// hunted for in a canvas.
await page.click('.tab[data-tab="build"]');
await page.waitForTimeout(600);
await page.locator('.made-search').evaluate((n) => n.scrollIntoView());
await page.waitForTimeout(200);
await page.screenshot({ path: `${outDir}/10e-made-slots.png` });

// The list is searched rather than shown: forty odd things with four states
// apiece is not a menu to scroll.
if ((await page.locator('.made-slot').count()) !== 0) {
  problems.push('picture slots are listed before anything is searched for or filled');
}
const madeLabels = () =>
  page.$$eval('.made-slot .field-label', (n) => n.map((x) => x.firstChild.textContent.trim()));
await page.fill('.made-search input', 'smithy');
await page.waitForTimeout(400);
const smithy = await madeLabels();
if (smithy.length < 2 || !smithy[0].startsWith('Smithy')) {
  problems.push(`searching the pictures for "smithy" gave ${JSON.stringify(smithy.slice(0, 3))}`);
}
if (!smithy.some((l) => l.includes('after dark'))) {
  problems.push('a thing offers no picture for after dark');
}
await page.screenshot({ path: `${outDir}/10g-made-search.png` });
// Meaning, if a table was built for this list.
if ((await page.locator('.made-search .btn.toggle:has-text("Meaning")').count()) > 0) {
  await page.fill('.made-search input', 'lantern');
  await page.waitForTimeout(300);
  if ((await madeLabels()).length !== 0) {
    problems.push('"lantern" is spelled like nothing here and should find nothing by letters');
  }
  await page.click('.made-search .btn.toggle:has-text("Meaning")');
  await page.waitForTimeout(400);
  const meant = await madeLabels();
  if (!meant.some((l) => l.startsWith('Lamp post'))) {
    problems.push(`"lantern" by meaning gave ${JSON.stringify(meant.slice(0, 3))}`);
  }
  console.log(`made pictures: lantern -> ${meant[0]}`);
  await page.click('.made-search .btn.toggle:has-text("Meaning")');
}
await page.fill('.made-search input', '');
await page.waitForTimeout(300);
// The lists above this button redraw as the settlement runs, and a redraw
// between aiming and pressing can carry the press onto whatever moved under
// it. The button says whether the press landed, so a miss is pressed again
// rather than reported as a hundred missing slots.
const everySlot = '.made-search .btn.toggle:has-text("Every slot")';
await page.click(everySlot);
if ((await page.getAttribute(everySlot, 'aria-pressed')) !== 'true') {
  await page.click(everySlot);
}
// The panel is rebuilt on the next frame and there are a hundred and thirty
// slots to draw, so this waits for them rather than guessing at how long a
// busy settlement takes to get round to it.
await page
  .waitForFunction(() => document.querySelectorAll('.made-slot').length >= 100, null, {
    timeout: 8000,
  })
  .catch(() => {});
const slots = await page.locator('.made-slot').count();
if (slots < 100) {
  problems.push(`Every slot listed ${slots} of them`);
}
await page.click('.made-search .btn.toggle:has-text("Every slot")');
await page.waitForTimeout(400);

// Growing the map: the town carries on, on a larger map, with no rebuild.
await page.click('.tab:text-is("Land")');
await page.waitForTimeout(500);
const colsBox = '#panel-body [data-find="columns-x"] input[type=number]';
const wasCols = Number(await page.inputValue(colsBox));
const townBefore = await page.evaluate(() =>
  document.getElementById('statusbar').textContent.split('   ')[0]
);
const addCols = '#panel-body [data-find="add-columns"] input[type=number]';
await page.fill(addCols, '24');
await page.dispatchEvent(addCols, 'input');
await page.click('#panel-body .btn:text-is("Grow the map")');
// The wilderness warmup on the new ground blocks the thread for a moment.
await page.waitForTimeout(12000);
const nowCols = Number(await page.inputValue(colsBox));
if (nowCols !== wasCols + 24) {
  problems.push(`growing the map left the width at ${nowCols}, not ${wasCols + 24}`);
}
if (!(await page.evaluate(() => document.getElementById('restart-bar').hasAttribute('hidden')))) {
  problems.push('growing the map left a rebuild waiting');
}
const townAfter = await page.evaluate(() =>
  document.getElementById('statusbar').textContent.split('   ')[0]
);
if (townAfter !== townBefore) {
  problems.push(`the town was ${townBefore} and is ${townAfter} after the map grew`);
}
await page.screenshot({ path: `${outDir}/11b-grown.png` });

// Left alone, the map takes the whole window on its own, and the first sign of
// life hands the menus back. Not the browser's fullscreen: this is the page
// folding its own chrome away, which is the only kind an untouched window can
// have.
const idleBox = '#panel-body [data-find="fullscreen-when-idle-s"] input[type=number]';
await page.fill(idleBox, '2');
await page.dispatchEvent(idleBox, 'input');
// The move is what re-arms the wait with the value just typed, and is the last
// thing that touches the page before it is left alone.
await page.mouse.move(900, 500);
await page.waitForTimeout(4000);
if (!(await page.evaluate(() => document.body.classList.contains('settled')))) {
  problems.push('the map never took the window after being left alone');
}
if (!(await page.evaluate(() => document.body.classList.contains('stage-only')))) {
  problems.push('settling in left the menus up');
}
await page.screenshot({ path: `${outDir}/11c-settled.png` });
await page.mouse.move(700, 400);
await page.waitForTimeout(600);
if (await page.evaluate(() => document.body.classList.contains('settled'))) {
  problems.push('moving the pointer did not hand the menus back');
}
await page.fill(idleBox, '0');
await page.dispatchEvent(idleBox, 'input');
await page.mouse.move(710, 410);

await page.click('.mode[data-mode="sprites"]');
await page.waitForTimeout(800);
await page.click('.tab[data-tab="sheet"]');
await page.waitForTimeout(500);
await page.selectOption('#panel-body [data-find="or-a-made-thing"] select', 'hut');
await page.click('#panel-body .btn:text-is("Use for that")');
// The note lands with the click; read it well before the autosave that the
// click also queued replaces it with "saved <time>" 600ms later.
await page.waitForTimeout(250);
if (!(await page.textContent('#save-note')).includes('hut')) {
  problems.push(`sending a sheet to the hut said "${(await page.textContent('#save-note')).trim()}"`);
}

await page.click('.mode[data-mode="settlement"]');
await page.waitForTimeout(3000);
await page.click('.tab[data-tab="build"]');
await page.waitForTimeout(600);
await page.fill('.made-search input', 'hut');
await page.waitForTimeout(400);
if ((await page.locator('.made-slot .btn.danger').count()) === 0) {
  problems.push('the hut has no picture after one was sent to it');
}
const picturesOn = await page.evaluate(() => {
  const label = [...document.querySelectorAll('#panel-body .field-label')].find((n) =>
    n.textContent.includes('Draw made things'),
  );
  const button = label?.closest('.field').querySelector('.btn.toggle');
  return button ? button.getAttribute('aria-pressed') === 'true' : null;
});
if (picturesOn !== true) problems.push('sending a picture did not turn pictures on');
// A filled slot says how large the picture comes out and carries the scale
// that changes it; a picture is drawn at its own size, never stretched to a box.
const madeSize = await page.textContent('.made-slot > .field-hint');
if (!/px, drawn [\d.]+x[\d.]+ cells/.test(madeSize ?? '')) {
  problems.push(`a filled picture slot said "${madeSize}" rather than what it draws`);
}
if ((await page.locator('.made-slot .field[data-find="scale"]').count()) === 0) {
  problems.push('a filled picture slot has no scale to set');
}
await page.locator('.made-slot').first().evaluate((n) => n.scrollIntoView());
await page.waitForTimeout(200);
await page.screenshot({ path: `${outDir}/10f-made-filled.png` });
// Clear it again, so the rest of the run looks like the rest of the run.
await page.click('.made-slot .btn.danger');
await page.waitForTimeout(500);
if ((await page.locator('.made-slot .btn.danger').count()) !== 0) {
  problems.push('clearing a picture left it filled');
}
await page.fill('.made-search input', '');
await page.waitForTimeout(300);

// Foliage over a person: three ways, and the amount only shows for the one it
// means anything for.
await page.click('.tab[data-tab="land"]');
await page.waitForTimeout(500);
const foliage = '#panel-body [data-find="foliage-over-people"] select';
const alphaShown = () => page.locator('#panel-body [data-find="how-much-foliage-is-left"]').count();
if ((await alphaShown()) !== 0) problems.push('the foliage amount shows with nothing to fade');
for (const mode of ['hatched', 'faded', 'solid']) {
  await page.selectOption(foliage, mode);
  await page.waitForTimeout(700);
  const shown = await alphaShown();
  if ((mode === 'faded') !== (shown === 1)) {
    problems.push(`the foliage amount is ${shown ? 'shown' : 'hidden'} for ${mode}`);
  }
  await page.screenshot({ path: `${outDir}/10d-foliage-${mode}.png` });
}

// The view menu: what the stage draws over the map is a dropdown in the top
// bar, and the label switches are one per category with walls on their own.
const pressed = (find) =>
  page.evaluate((f) => {
    const node = document.querySelector(`#view-body [data-find="${f}"]`);
    return node ? node.getAttribute('aria-pressed') : null;
  }, find);
const press = async (find) => {
  await openView();
  await page.click(`#view-body [data-find="${find}"]`);
};

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
await closeView();

// Moving people: with the switch on, a press on a person picks them up and
// the pointer carries them until it is let go. Where the people are on
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
// Presses every few pixels of the stage until the note says the press landed
// on somebody. `want` is what that note starts with, which is what tells
// picking somebody up from taking them over.
const sweepStage = (want = 'holding', release = false) =>
  page.evaluate(({ want, release }) => {
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
      if (note().includes(want)) {
        // A person picked up stays picked up until the drag that follows
        // puts them down; one taken over is not held by the pointer at all.
        if (release) send('pointerup', x, y);
        return { x, y, who: note() };
      }
      send('pointerup', x, y);
    }
  }
  return null;
  }, { want, release });
// People indoors are not on the map to be picked up, so a town that has
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
  problems.push('no person could be picked up anywhere on the stage');
} else {
  console.log(`picked up: ${sweep.who}`);
  if (!(await page.evaluate(() => document.body.classList.contains('holding')))) {
    problems.push('holding a person did not show in the pointer');
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
  if (!put.startsWith('put ')) problems.push(`putting a person down said "${put}"`);
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${outDir}/11c-put-down.png` });
}
await page.click('#move-people');
await page.waitForTimeout(200);
if (await page.evaluate(() => document.body.classList.contains('moving-people'))) {
  problems.push('turning the move people switch off left the stage picking people up');
}

// Cutting by hand: the switch is exclusive with moving people, a held press on
// something growing takes it down, and a press let go of too soon does not.
await page.click('#move-people');
await page.waitForTimeout(150);
await page.click('#harvest-mode');
await page.waitForTimeout(200);
if (!(await page.evaluate(() => document.body.classList.contains('harvesting')))) {
  problems.push('the harvest switch did not change what a press on the stage does');
}
if (await page.evaluate(() => document.body.classList.contains('moving-people'))) {
  problems.push('turning harvesting on left the stage picking people up as well');
}
if (
  (await page.evaluate(() => document.getElementById('move-people').getAttribute('aria-pressed'))) !==
  'false'
) {
  problems.push('the move people button still reads as on with harvesting on');
}

const stagePress = (x, y, type) =>
  page.evaluate(
    ({ x, y, type }) => {
      const canvas = document.getElementById('world-canvas');
      const r = canvas.getBoundingClientRect();
      canvas.dispatchEvent(
        new PointerEvent(type, {
          pointerId: 1,
          clientX: r.left + x * r.width,
          clientY: r.top + y * r.height,
          bubbles: true,
          button: 0,
          buttons: type === 'pointerup' ? 0 : 1,
        }),
      );
      return (document.getElementById('save-note').textContent || '').trim();
    },
    { x, y, type },
  );
const noteNow = () =>
  page.evaluate(() => (document.getElementById('save-note').textContent || '').trim());
const stageHover = (x, y) =>
  page.evaluate(
    ({ x, y }) => {
      const canvas = document.getElementById('world-canvas');
      const r = canvas.getBoundingClientRect();
      canvas.dispatchEvent(
        new PointerEvent('pointermove', {
          pointerId: 1,
          clientX: r.left + x * r.width,
          clientY: r.top + y * r.height,
          bubbles: true,
          buttons: 0,
        }),
      );
    },
    { x, y },
  );

// The pulse over what can be cut, and the firmer outline on whatever the
// pointer is over. Nothing to assert from out here beyond the frame surviving
// it, so this is a picture to look at.
await stageHover(0.5, 0.6);
await page.waitForTimeout(400);
await page.screenshot({ path: `${outDir}/11f-harvest-hover.png` });

// A press let go of at once is not a cut, however green the ground under it.
await stagePress(0.5, 0.6, 'pointerdown');
await page.waitForTimeout(90);
await stagePress(0.5, 0.6, 'pointerup');
await page.waitForTimeout(400);
if (/^cut /.test(await noteNow())) {
  problems.push('a press let go of at once still cut something down');
}

// Holding is what cuts. Where the plants are on screen is not knowable from
// out here, so the stage is swept a point at a time until something comes
// down. The clock stays stopped: a hand works whether the world is running or
// not, which is worth checking here rather than only in the tests.
let cutNote = null;
let cutAt = { x: 0.5, y: 0.6 };
for (let y = 0.3; y < 0.95 && !cutNote; y += 0.12) {
  for (let x = 0.1; x < 0.95 && !cutNote; x += 0.08) {
    await stagePress(x, y, 'pointerdown');
    await page.waitForTimeout(700);
    const note = await noteNow();
    await stagePress(x, y, 'pointerup');
    if (/^cut /.test(note)) {
      cutNote = note;
      cutAt = { x, y };
    }
  }
}
if (!cutNote) {
  problems.push('holding the pointer over the map cut nothing anywhere on it');
} else {
  console.log(`cut by hand: ${cutNote}`);
  if (!/on the ground$/.test(cutNote)) {
    problems.push(`a cut did not say what it left behind: "${cutNote}"`);
  }
  // Part way through a cut is when there is a bar to see.
  await stagePress(cutAt.x, cutAt.y, 'pointerdown');
  await page.waitForTimeout(260);
  await page.screenshot({ path: `${outDir}/11g-harvest-holding.png` });
  await stagePress(cutAt.x, cutAt.y, 'pointerup');
}
await page.click('#harvest-mode');
await page.waitForTimeout(200);
if (await page.evaluate(() => document.body.classList.contains('harvesting'))) {
  problems.push('turning the harvest switch off left the stage cutting');
}

// Adding people: the third exclusive press switch. A press on the map sets a
// new person down there; the head count says whether anybody arrived.
const headCount = async () => {
  const status = await page.evaluate(() => document.getElementById('statusbar').textContent);
  const m = /people (\d+)/.exec(status);
  return m ? Number(m[1]) : -1;
};
await page.click('#harvest-mode');
await page.waitForTimeout(150);
await page.click('#add-people');
await page.waitForTimeout(200);
if (!(await page.evaluate(() => document.body.classList.contains('adding-people')))) {
  problems.push('the add people switch did not change what a press on the stage does');
}
if (await page.evaluate(() => document.body.classList.contains('harvesting'))) {
  problems.push('turning add people on left the stage cutting as well');
}
const headsBefore = await headCount();
await stagePress(0.5, 0.55, 'pointerdown');
await stagePress(0.5, 0.55, 'pointerup');
await page.waitForTimeout(600);
const headsAfter = await headCount();
if (headsAfter !== headsBefore + 1) {
  problems.push(`a press with add people on went from ${headsBefore} heads to ${headsAfter}`);
}
if (!(await page.evaluate(() => document.getElementById('save-note').textContent)).includes('wandered in')) {
  problems.push('nobody said who arrived');
}
console.log(`added a person: ${(await page.evaluate(() => document.getElementById('save-note').textContent)).trim()}`);
await page.click('#add-people');
await page.waitForTimeout(200);
if (await page.evaluate(() => document.body.classList.contains('adding-people'))) {
  problems.push('turning the add people switch off left the stage adding people');
}

// Looking inside: the fourth exclusive switch. A press on a building lands
// its card on the Build panel; where the buildings are on screen is not
// knowable from out here, so the stage is swept the way the person pick-up
// sweep does it.
await page.click('#look-inside');
await page.waitForTimeout(200);
if (!(await page.evaluate(() => document.body.classList.contains('inspecting')))) {
  problems.push('the look inside switch did not change what a press on the stage does');
}
const inspectSweep = await page.evaluate(() => {
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
      const n = note();
      send('pointerup', x, y);
      if (n.startsWith('looking inside')) return n;
    }
  }
  return null;
});
if (!inspectSweep) {
  problems.push('no building could be looked inside anywhere on the stage');
} else {
  console.log(`looked inside: ${inspectSweep}`);
  await page.waitForTimeout(300);
  if ((await page.getAttribute('.tab.active', 'data-tab')) !== 'build') {
    problems.push('looking inside a building did not open the Build panel');
  }
  const card = page.locator('[data-group="Looking inside"]');
  if (!(await card.isVisible())) {
    problems.push('the building card is not showing on the Build panel');
  } else {
    if ((await card.locator('.stat').count()) < 3) {
      problems.push('the building card says almost nothing');
    }
    await page.screenshot({ path: `${outDir}/11h-look-inside.png` });
    // Condemning it: the card is where the order is given, and the state line
    // is what says the town took it.
    const order = card.locator('.btn.danger:text-is("Pull it down"), .btn:text-is("Call it off")');
    if ((await order.count()) === 0) {
      problems.push('the building card offers no way to have it taken down');
    } else if ((await order.textContent()) === 'Pull it down') {
      await order.click();
      await page.waitForTimeout(700);
      const state = await card.locator('.stat').first().textContent();
      if (!/condemned/.test(state ?? '')) {
        problems.push(`condemning a building left the card reading "${state}"`);
      }
      await page.screenshot({ path: `${outDir}/11h2-condemned.png` });
      // And it can be called off again, which is what the button says now.
      const spare = card.locator('.btn:text-is("Let it stand")');
      if ((await spare.count()) === 0) {
        problems.push('a condemned building cannot be spared');
      } else {
        await spare.click();
        await page.waitForTimeout(700);
        const back = await card.locator('.stat').first().textContent();
        if (/condemned/.test(back ?? '')) {
          problems.push('letting it stand left it condemned');
        }
      }
    }
    await card.locator('.btn:text-is("Done looking")').click();
    await page.waitForTimeout(300);
    if (await card.isVisible()) {
      problems.push('Done looking left the card up');
    }
  }
}
await page.click('#look-inside');
await page.waitForTimeout(150);

// Taking a person over: the switch is only on the toolbar while the
// experiment behind it is on, and what it puts up is stage chrome rather than
// a panel - a stick and the four things a person can be asked to do.
await page.click('.tab[data-tab="experimental"]');
await page.waitForTimeout(500);
if ((await page.locator('#take-over').count()) !== 0) {
  problems.push('the take over switch was on the toolbar with the experiment off');
}
// Zones from a picture: a four pixel image, red down one side and blue down
// the other, laid over the map. Dragging a box over the red half and applying
// a zone to it should take about half the cells in the box and nothing else.
await page.click('.tab[data-tab="land"]');
await page.waitForTimeout(500);
const zoneGroup = page.locator('#panel-body .group:has-text("Zones from a picture")');
if ((await zoneGroup.count()) === 0) {
  problems.push('the Land panel has no way to draw zones from a picture');
} else {
  const RED_AND_BLUE =
    'iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAFUlEQVR4nGO4YGAARAYJF4CIgTgOABDSFIGliA40AAAAAElFTkSuQmCC';
  await zoneGroup.locator('input[type=file]').setInputFiles({
    name: 'land.png',
    mimeType: 'image/png',
    buffer: Buffer.from(RED_AND_BLUE, 'base64'),
  });
  await page.waitForTimeout(900);
  const canvas = zoneGroup.locator('canvas.landscape');
  if ((await canvas.count()) === 0) {
    problems.push('the dropped picture was not laid over the map');
  } else {
    // Press on the red half and drag the box across the whole picture. The
    // events are dispatched on the canvas itself rather than driven with the
    // mouse: the panel scrolls, and a canvas that is half above the fold has a
    // bounding box the pointer cannot reach.
    await canvas.evaluate((node) => {
      const r = node.getBoundingClientRect();
      const send = (type, fx, fy, buttons) =>
        node.dispatchEvent(
          new PointerEvent(type, {
            pointerId: 1,
            clientX: r.left + r.width * fx,
            clientY: r.top + r.height * fy,
            bubbles: true,
            button: 0,
            buttons,
          }),
        );
      send('pointerdown', 0.1, 0.5, 1);
      send('pointermove', 0.6, 0.9, 1);
      send('pointermove', 0.98, 0.98, 1);
      send('pointerup', 0.98, 0.98, 0);
    });
    await page.waitForTimeout(400);
    const readout = (await page.textContent('#zone-readout')) ?? '';
    const hit = readout.match(/^(\d+) cells match, of (\d+)/);
    if (!hit) {
      problems.push(`dragging a box on the picture read "${readout}"`);
    } else if (!(Number(hit[1]) > 0 && Number(hit[1]) < Number(hit[2]))) {
      problems.push(`the color threshold took ${hit[1]} of ${hit[2]} cells, not some of them`);
    }
    await zoneGroup.locator('[data-find="make-it"] select').selectOption('bare');
    await page.waitForTimeout(200);
    await zoneGroup.locator('.btn:text-is("Apply to the map")').click();
    await page.waitForTimeout(500);
    const said = (await page.textContent('#save-note')).trim();
    if (!/cells are growth: nothing/.test(said)) {
      problems.push(`applying a zone said "${said}"`);
    }
    await page.screenshot({ path: `${outDir}/11l-zones.png` });
    await zoneGroup.locator('.btn:text-is("Forget the picture")').click();
    await page.waitForTimeout(400);
  }
}

// Placing things by hand: the menu is on the Build panel and the press is the
// stage's, the same as every other switch over the map.
await page.click('.tab[data-tab="build"]');
await page.waitForTimeout(500);
await page.click('#place-thing');
await page.waitForTimeout(500);
const placeMenu = page.locator('#panel-body [data-find="put-down"] select');
if ((await placeMenu.count()) === 0) {
  problems.push('turning Place on brought up no placing menu');
} else {
  // Scenery: a press on the sky puts a hill up behind the map, and the Land
  // panel is where it is then adjusted and taken down again.
  await placeMenu.selectOption('scenery');
  await page.waitForTimeout(400);
  // Down the middle until a press lands in the sky band: where that is on
  // screen depends on the camera, and the letterbox above the map belongs to
  // the camera rather than to the sky.
  const putUp = await page.evaluate(() => {
    const canvas = document.getElementById('world-canvas');
    const r = canvas.getBoundingClientRect();
    const note = () => (document.getElementById('save-note').textContent || '').trim();
    const send = (type, x, y, buttons) =>
      canvas.dispatchEvent(
        new PointerEvent(type, {
          pointerId: 1,
          clientX: x,
          clientY: y,
          bubbles: true,
          button: 0,
          buttons,
        }),
      );
    const x = r.left + r.width * 0.5;
    for (let y = r.top + 4; y < r.bottom - 4; y += 8) {
      send('pointerdown', x, y, 1);
      send('pointerup', x, y, 0);
      if (note().includes('put up')) return note();
    }
    return note();
  });
  await page.waitForTimeout(500);
  if (!/put up/.test(putUp)) {
    problems.push(`pressing the sky with scenery in hand said "${putUp}"`);
  }
  await page.screenshot({ path: `${outDir}/11m-scenery.png` });
  await page.click('.tab[data-tab="land"]');
  await page.waitForTimeout(600);
  const behind = page.locator('#panel-body .group:has-text("Behind the map")');
  if ((await behind.locator('.btn.danger:text-is("Take it down")').count()) === 0) {
    problems.push('the piece that went up is not listed on the Land panel');
  } else {
    await behind.locator('.btn.danger:text-is("Take it down")').first().click();
    await page.waitForTimeout(500);
    if ((await behind.locator('.btn.danger:text-is("Take it down")').count()) !== 0) {
      problems.push('taking a piece of scenery down left it standing');
    }
  }
  await page.click('.tab[data-tab="build"]');
  await page.waitForTimeout(500);

  await placeMenu.selectOption('load');
  await page.waitForTimeout(500);
  const amount = '#panel-body [data-find="how-much"] input.num';
  if ((await page.locator(amount).count()) === 0) {
    problems.push('a load has nothing to say how much of it to put down');
  }
  const before = (await page.textContent('#save-note')).trim();
  await page.mouse.click(stage.x + stage.width * 0.5, stage.y + stage.height * 0.6);
  await page.waitForTimeout(400);
  const said = (await page.textContent('#save-note')).trim();
  if (said === before || !/ground|nowhere|off the map/.test(said)) {
    problems.push(`placing a load said "${said}"`);
  }
  await page.screenshot({ path: `${outDir}/11k-placing.png` });
}
await page.click('#place-thing');
await page.waitForTimeout(200);
await page.click('.tab[data-tab="experimental"]');
await page.waitForTimeout(400);
await page.click('#panel-body [data-find="try-the-unfinished-things"] .btn');
await page.waitForTimeout(500);
await page.click('#panel-body [data-find="let-a-person-be-taken-over"] .btn');
await page.waitForTimeout(600);
if ((await page.locator('#take-over').count()) === 0) {
  problems.push('turning the experiment on did not put the take over switch up');
} else {
  await pause();
  await page.click('#take-over');
  await page.waitForTimeout(200);
  const drove = await sweepStage(' is yours', true);
  if (!drove || !/ is yours/.test(drove.who)) {
    problems.push(`no person could be taken over: ${drove ? drove.who : 'nobody found'}`);
  } else {
    console.log(`took over: ${drove.who}`);
    await page.waitForTimeout(400);
    const hud = page.locator('#stage-hud');
    if (!(await hud.isVisible())) {
      problems.push('taking a person over put no chrome over the map');
    }
    if ((await hud.locator('.stick').count()) === 0) {
      problems.push('the stick is missing with the stick switch on');
    }
    const acts = await hud.locator('.hud-acts .btn').count();
    if (acts < 5) {
      problems.push(`the driving row has ${acts} buttons rather than the four and a let go`);
    }
    await page.screenshot({ path: `${outDir}/11j-driving.png` });
    // A press on one of them says what happened, whether or not there was
    // anything in reach to do it to.
    await hud.locator('.btn:has-text("Pick up"), .btn:has-text("Put down")').first().click();
    await page.waitForTimeout(300);
    const said = (await page.textContent('#save-note')).trim();
    if (said.length === 0) {
      problems.push('pressing a driving button said nothing at all');
    }
    // The keys steer: hold one down for a moment with the world running and
    // the person should have moved.
    await resume();
    await page.keyboard.down('d');
    await page.waitForTimeout(700);
    await page.keyboard.up('d');
    await page.waitForTimeout(200);
    await pause();
    await hud.locator('.btn:text-is("Let go")').click();
    await page.waitForTimeout(400);
    if (await hud.isVisible()) {
      problems.push('letting go left the driving chrome over the map');
    }
  }
  await page.click('#take-over');
  await page.waitForTimeout(150);
}

// The map editor: a third page of the sprite editor, which only exists while
// the experiment is on. It is the pixel editor with a legend for a palette, so
// what is checked is that a press on the stage lands as a zone rather than as
// a color.
await page.click('.mode:text-is("Sprite editor")');
await page.waitForTimeout(700);
if ((await page.locator('.tab').allTextContents()).join() !== 'Draw,Sheet,Map') {
  problems.push('the map page is missing with the experiment on');
} else {
  await page.click('.tab[data-tab="map"]');
  await page.waitForTimeout(600);
  // The tally is one stat per brush, name and count. Read whole rows, and
  // only the tally's own section: the "nothing yet" an empty page shows is a
  // value rather than a name, and a loaded picture puts a stat of its own on
  // the panel.
  const painted = () =>
    page.evaluate(() =>
      [
        ...document.querySelectorAll('#panel-body details.group[data-group="Applying it"] .stat'),
      ]
        .map((n) => n.textContent)
        .join(' | '),
    );
  if (!/nothing yet/.test(await painted())) {
    problems.push('the map page opened with something already painted on it');
  }
  await page.click('#panel-body .chip:has-text("Water")');
  await page.waitForTimeout(200);
  const canvas = await page.locator('#world-canvas').boundingBox();
  await page.mouse.move(canvas.x + canvas.width * 0.45, canvas.y + canvas.height * 0.5);
  await page.mouse.down();
  await page.mouse.move(canvas.x + canvas.width * 0.55, canvas.y + canvas.height * 0.55, {
    steps: 12,
  });
  await page.mouse.up();
  await page.waitForTimeout(400);
  if (!/Water/.test(await painted())) {
    problems.push(`a stroke on the map page painted nothing: ${await painted()}`);
  }
  await page.screenshot({ path: `${outDir}/19-map-editor.png` });
  // Applying it to the running settlement should say how much it drew.
  await page.click('#panel-body .btn:text-is("Apply to the map")');
  await page.waitForTimeout(600);
  const drew = (await page.textContent('#save-note')).trim();
  if (!/cells drawn/.test(drew)) {
    problems.push(`applying the drawn map said "${drew}"`);
  }
  await page.click('#panel-body .btn:text-is("Wipe the drawing")');
  await page.waitForTimeout(400);
  if (!/nothing yet/.test(await painted())) {
    problems.push('wiping the drawing left something on it');
  }
}
await page.click('.mode:text-is("Settlement")');
await page.waitForTimeout(1500);
await page.click('.tab[data-tab="experimental"]');
await page.waitForTimeout(400);

// Put the experiments back where they were.
await page.click('#panel-body [data-find="let-a-person-be-taken-over"] .btn');
await page.waitForTimeout(300);
await page.click('#panel-body [data-find="try-the-unfinished-things"] .btn');
await page.waitForTimeout(300);
await resume();

// Copy and Save in a section head: a section of settings carries both, a
// section that is only a readout carries neither, and Save writes a file of
// that section alone. The press has to land on the button rather than on the
// head it sits in, so the section is checked to be still open afterwards.
// The Land panel has one of each: View is settings, This land is a readout.
await page.click('.tab[data-tab="land"]');
await page.waitForTimeout(500);
const viewTools = '#panel-body details.group[data-group="View"] .group-tools';
if ((await page.locator(`${viewTools} .btn`).count()) !== 2) {
  problems.push('a section of settings has no copy and save buttons');
}
if ((await page.locator('#panel-body details.group[data-group="This land"] .group-tools').count()) !== 0) {
  problems.push('a section with nothing but a readout in it offered to save it');
}
const sectionFile = await grab(`${viewTools} .btn:text-is("Save")`);
if (!/\.json$/.test(sectionFile.name)) {
  problems.push(`saving a section gave ${sectionFile.name}, which is not a json file`);
}
try {
  const parsed = JSON.parse(sectionFile.bytes.toString());
  if (parsed.section !== 'View') {
    problems.push(`the saved section says it is "${parsed.section}"`);
  }
  const cover = (parsed.fields || []).find((f) => f.key === 'cloud-cover');
  if (typeof cover?.value !== 'number') {
    problems.push('the saved section does not carry the cloud cover as a number');
  }
} catch (err) {
  problems.push(`the saved section is not readable json: ${err.message}`);
}
if ((await page.getAttribute('#panel-body details.group[data-group="View"]', 'open')) === null) {
  problems.push('pressing Save in a section head folded the section');
}

// The sky past the map's edge: flip the space clouds on, let a few frames
// draw the letterbox as sky, and flip them back. The console listener is what
// fails this if the pattern path throws.
const spaceClouds = '#panel-body [data-find^="clouds-past"] .btn';
await page.click(spaceClouds);
await page.waitForTimeout(600);
await page.screenshot({ path: `${outDir}/11i-space-clouds.png` });
await page.click(spaceClouds);
await page.waitForTimeout(200);
await resume();

// Back to the lab and in again: both sims have to survive the switch.
await page.click('.mode:text-is("Plant lab")');
await page.waitForTimeout(1200);
await page.click('.mode:text-is("Settlement")');
await page.waitForTimeout(2000);
await page.screenshot({ path: `${outDir}/12-settlement-return.png` });

// The menu's edge drags. The width lands in a root custom property in rem so
// it rides along with the text scale, and a double press puts the default
// width back.
const panelWidth = async () => (await page.locator('.panel').boundingBox()).width;
const panelW0 = await panelWidth();
const grip = await page.locator('#panel-resize').boundingBox();
await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
await page.mouse.down();
await page.mouse.move(grip.x + grip.width / 2 + 160, grip.y + grip.height / 2, { steps: 8 });
await page.mouse.up();
await page.waitForTimeout(300);
const panelW1 = await panelWidth();
if (panelW1 - panelW0 < 100) {
  problems.push(`dragging the menu edge did not widen it (${panelW0} -> ${panelW1})`);
}
if (!(await page.evaluate(() => document.documentElement.style.getPropertyValue('--panel-w').endsWith('rem')))) {
  problems.push('the dragged menu width is not kept in rem');
}
await page.dblclick('#panel-resize');
await page.waitForTimeout(500);
const panelW2 = await panelWidth();
if (Math.abs(panelW2 - panelW0) > 8) {
  problems.push(`a double press did not put the default width back (${panelW0} -> ${panelW2})`);
}

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
  () => !document.getElementById('find-meaning').hasAttribute('hidden'),
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

// Text scale reaches the whole page through the root font size, but only once
// the slider is let go: what it resizes is the page the slider is sitting in,
// so applying it mid drag walks it out from under the pointer. The box beside
// it reads the value on the way, and is a way in of its own.
const scaled = await page.evaluate(() => {
  const px = () => parseFloat(getComputedStyle(document.documentElement).fontSize);
  const input = document.getElementById('ui-scale');
  const box = document.getElementById('ui-scale-box');
  const before = px();
  input.value = '1.5';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  const dragging = px() / before;
  const reads = box.value;
  input.dispatchEvent(new Event('change', { bubbles: true }));
  const released = px() / before;
  box.value = '125';
  box.dispatchEvent(new Event('change', { bubbles: true }));
  const typed = px() / before;
  const follows = input.value;
  box.value = '100';
  box.dispatchEvent(new Event('change', { bubbles: true }));
  return { dragging, reads, released, typed, follows };
});
if (scaled.dragging !== 1) {
  problems.push(`the page resized by ${scaled.dragging.toFixed(2)} while the slider was held`);
}
if (scaled.reads !== '150') problems.push(`the size box read ${scaled.reads} mid drag, not 150`);
if (scaled.released < 1.4) {
  problems.push(`letting the slider go moved the root size by ${scaled.released.toFixed(2)}, not 1.5`);
}
if (Math.abs(scaled.typed - 1.25) > 0.02) {
  problems.push(`typing 125 into the size box moved the root size by ${scaled.typed.toFixed(2)}`);
}
if (scaled.follows !== '1.25') {
  problems.push(`the slider read ${scaled.follows} after the box was typed into, not 1.25`);
}

// The settlement survives a reload. The page is told it is going away, which
// is what writes the world down between the timed saves, and then reloaded:
// coming back has to pick the same town up on the same day rather than found
// a new one. This goes last because a reload throws away everything above it.
await page.click('.mode[data-mode="settlement"]');
await page.waitForTimeout(1200);
const dayOf = async () => {
  const line = await page.evaluate(() => document.getElementById('statusbar').textContent);
  const hit = /day (\d+)/.exec(line);
  return hit ? Number(hit[1]) : -1;
};
const townOf = async () =>
  (await page.evaluate(() => document.getElementById('statusbar').textContent)).split(' ')[0];
const beforeDay = await dayOf();
const beforeTown = await townOf();
await page.evaluate(() => window.dispatchEvent(new Event('pagehide')));
await page.waitForTimeout(300);
const saved = await page.evaluate(() => localStorage.getItem('grow.settlement.v1')?.length ?? 0);
if (!saved) {
  problems.push('leaving the page did not write the settlement down');
}
await page.reload({ waitUntil: 'networkidle' });
await page.waitForTimeout(600);
await page.click('.mode[data-mode="settlement"]');
await page.waitForTimeout(2500);
const afterDay = await dayOf();
const afterTown = await townOf();
const note = await page.evaluate(() => document.getElementById('save-note').textContent);
if (afterTown !== beforeTown || afterDay < beforeDay) {
  problems.push(
    `the settlement did not survive a reload: ${beforeTown} day ${beforeDay} came back as ` +
      `${afterTown} day ${afterDay}`,
  );
} else {
  console.log(`reload: ${afterTown} picked up on day ${afterDay} (${(saved / 1e6).toFixed(2)} MB)`);
}
if (!/day/.test(note)) {
  problems.push(`the note after a reload read "${note}", not the day it picked up on`);
}
await page.screenshot({ path: `${outDir}/18-reload.png` });

await browser.close();
server.kill();

if (problems.length) {
  console.error(`problems (${problems.length}):`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`no console errors. screenshots in ${outDir}`);
