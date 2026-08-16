// Loads the tool in a headless browser, clicks through every tab, and reports
// any console error or uncaught exception. Writes screenshots next to the
// output path given as the first argument.
//
//   bun run tools/uicheck.js /tmp/shots

import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';

const outDir = process.argv[2] || '/tmp/grow-shots';
mkdirSync(outDir, { recursive: true });

const PORT = 5199;
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

// Run the simulation for a few seconds at speed so the stage has content.
const playBtn = page.locator('.toolbar-row .btn').first();
if ((await playBtn.textContent()).trim() === 'Play') await playBtn.click();
await page.waitForTimeout(300);
await page.evaluate(() => {
  const slider = document.querySelectorAll('.toolbar-row input[type=range]')[0];
  slider.value = '16';
  slider.dispatchEvent(new Event('input', { bubbles: true }));
});
await page.waitForTimeout(4000);
await page.screenshot({ path: `${outDir}/01-materials.png` });

for (const tab of ['Shading', 'Species', 'World']) {
  await page.click(`.tab:text-is("${tab}")`);
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${outDir}/0${['Shading', 'Species', 'World'].indexOf(tab) + 2}-${tab.toLowerCase()}.png` });
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
console.log(`status: ${stats}`);

await browser.close();
server.kill();

if (problems.length) {
  console.error(`problems (${problems.length}):`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`no console errors. screenshots in ${outDir}`);
