// Static file server. A WebAssembly module cannot be loaded over file://, so
// the tool is served over HTTP instead. Run with: bun run serve.js
import { existsSync, statSync } from 'node:fs';
import { join, normalize, extname } from 'node:path';

const ROOT = import.meta.dir;
const PORT = Number(process.env.PORT || 5173);

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
};

Bun.serve({
  port: PORT,
  fetch(req) {
    const url = new URL(req.url);
    let path = decodeURIComponent(url.pathname);
    if (path.endsWith('/')) path += 'index.html';
    const full = join(ROOT, normalize(path).replace(/^(\.\.[/\\])+/, ''));
    if (!full.startsWith(ROOT) || !existsSync(full) || statSync(full).isDirectory()) {
      return new Response('not found', { status: 404 });
    }
    return new Response(Bun.file(full), {
      headers: { 'content-type': TYPES[extname(full)] || 'application/octet-stream' },
    });
  },
});

console.log(`grow: http://localhost:${PORT}/`);
