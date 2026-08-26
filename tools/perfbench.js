// Measures how long a settlement frame takes in a real browser, at a range of
// zoom levels, with the simulation running and then stopped.
//
//   bun run tools/perfbench.js [cols] [rows] [warmSeconds]
//
// The Rust frame runs inside requestAnimationFrame, so a callback registered
// after it runs once it has finished: the frame timestamp subtracted from the
// time then is the work the frame did, canvas upload included, and unlike the
// interval between frames it is not pinned to the refresh rate.
//
// SHOT_DIR, if set, gets a screenshot of the stage per measurement.

import { chromium } from 'playwright-core';

const COLS = Number(process.argv[2] || 384);
const ROWS = Number(process.argv[3] || 192);
const WARM = Number(process.argv[4] || 25);
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
page.on('pageerror', (err) => console.error(`page error: ${err.message}`));

await page.goto(`http://localhost:${PORT}/`, { waitUntil: 'networkidle' });
await page.waitForTimeout(600);

await page.evaluate(([cols, rows]) => {
  const key = 'grow.project.v1';
  const raw = localStorage.getItem(key);
  const s = raw ? JSON.parse(raw) : { version: 3, civ: { world: {} } };
  s.version = 3;
  s.civ = s.civ || {};
  s.civ.world = s.civ.world || {};
  s.civ.world.cols = cols;
  s.civ.world.rows = rows;
  localStorage.setItem(key, JSON.stringify(s));
}, [COLS, ROWS]);
await page.reload({ waitUntil: 'networkidle' });
await page.waitForTimeout(600);

await page.click('#modes >> text=Settlement');
await page.waitForTimeout(2000);

const play = page.locator('.stage-toolbar .btn').first();
if ((await play.textContent()).trim() === 'Play') await play.click();
await page.evaluate(() => {
  const slider = document.querySelectorAll('.stage-toolbar input[type=range]')[0];
  if (slider) {
    slider.value = slider.max;
    slider.dispatchEvent(new Event('input', { bubbles: true }));
  }
});
await page.waitForTimeout(WARM * 1000);
console.log(await page.textContent('#statusbar'));

await page.evaluate(() => {
  // Registered after the wasm frame callback, so it runs once that has
  // finished: now() - ts is the work the frame did, not the vsync interval.
  window.__samples = [];
  const tick = (ts) => {
    window.__samples.push(performance.now() - ts);
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
});

const box = await page.locator('#world-canvas').boundingBox();
const cx = box.x + box.width / 2;
const cy = box.y + box.height / 2;

async function setZoom(z) {
  await page.evaluate((v) => {
    const el = document.getElementById('zoom-input');
    el.value = String(v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  }, z);
  await page.waitForTimeout(200);
}

async function wheelOut(n) {
  for (let i = 0; i < n; i++) {
    await page.mouse.move(cx, cy);
    await page.mouse.wheel(0, 200);
    await page.waitForTimeout(40);
  }
}

const SHOTS = process.env.SHOT_DIR;

async function sample(label) {
  await page.evaluate(() => { window.__samples.length = 0; });
  await page.waitForTimeout(3000);
  const out = await page.evaluate(() => {
    const s = window.__samples.slice().sort((a, b) => a - b);
    return { n: s.length, p50: s[Math.floor(s.length / 2)], p90: s[Math.floor(s.length * 0.9)] };
  });
  const status = await page.textContent('#statusbar');
  const detail = (status.match(/(\w+) detail/) || [])[1] || '';
  const zoom = await page.textContent('#zoom-readout');
  console.log(
    `${label.padEnd(9)} zoom ${String(zoom).padEnd(7)} ${detail.padEnd(8)}`
    + ` frame work p50 ${out.p50.toFixed(2)} ms  p90 ${out.p90.toFixed(2)} ms  (n=${out.n})`
  );
  if (SHOTS) {
    const name = label.replace(/\s+/g, '-');
    await page.locator('#world-canvas').screenshot({ path: `${SHOTS}/${name}.png` });
  }
}

for (const z of [4, 2, 1, 0.5]) {
  await setZoom(z);
  await sample(`zoom ${z}`);
}
await wheelOut(12);
await sample('zoomed out');

// Again with the simulation stopped, which leaves only the drawing.
const pause = page.locator('.stage-toolbar .btn').first();
if ((await pause.textContent()).trim() === 'Pause') await pause.click();
await page.waitForTimeout(300);
await sample('out still');
await setZoom(0.5);
await sample('0.5 still');
await setZoom(2);
await sample('2x still');

await browser.close();
server.kill();
