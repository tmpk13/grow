---
name: grow-checks
description: How to run every check in the grow repo (Rust/WebAssembly plant lab and settlement) - cargo tests, the headless smoke binaries, and the Playwright browser checks. Use before claiming a change to this repo works, and whenever a browser check fails to launch, cargo test refuses to build, or the page loads stale WebAssembly.
---

# Testing grow

Five kinds of check, in the order they are worth running. Everything is driven
from `package.json`; run the scripts rather than the commands behind them.

| What | Command | Needs |
| --- | --- | --- |
| Unit and integration tests | `bun run test` | nothing |
| Plant world smoke | `bun run check` | nothing |
| Settlement smoke | `bun run check:civ -- 40 /tmp/civ.ppm` | nothing |
| Menu index | `bun run check:menu` | built wasm, `CHROMIUM_PATH` |
| Whole page | `bun run check:ui` | built wasm, `CHROMIUM_PATH` |
| Frame timings | `bun run check:perf` | built wasm, `CHROMIUM_PATH` |

Lint with `cd rust && cargo clippy --profile reltest --all-targets`, and again
with `cargo clippy --target wasm32-unknown-unknown --lib`: `app.rs`, `render.rs`
and everything under `ui/` are compiled **only** for wasm, so a host-target
clippy or check says nothing about them. A change to those files that has not
been built for wasm has not been checked at all.

## The four traps

**1. The browser checks cannot find a browser on their own.**

`playwright-core` looks for the exact browser revision it was built against,
and what is in the cache is an older one. The failure reads as
`Executable doesn't exist at .../chromium_headless_shell-1234/...` and tells
you to run `npx playwright install`; do not, it downloads a browser this
machine does not need. Point the checks at what is already installed:

```sh
export CHROMIUM_PATH=$HOME/.cache/ms-playwright/chromium_headless_shell-1181/chrome-linux/headless_shell
bun run check:ui
```

The revision number in that path is whatever `ls ~/.cache/ms-playwright`
reports; it is not pinned to the one above. Every browser check reads
`CHROMIUM_PATH` (`tools/uicheck.js`, `tools/menuindex.js`, `tools/perfbench.js`
all pass it to `chromium.launch`), so exporting it once covers all of them.

**2. The browser checks load `pkg/`, not your source.**

`index.html` imports `./pkg/grow.js`, which is the last `bun run build` output.
Run `bun run build` before any browser check or the page under test is the
previous version of the program, and the check will pass or fail for reasons
that have nothing to do with the change. This is the one place a release build
is expected: `build` compiles the wasm library in release and runs
`wasm-bindgen`, and there is no debug path to the page.

Do not start `serve.js` yourself first. Each browser check spawns its own
server on a random port so a server left over from an earlier run cannot serve
stale files.

**3. `cargo test` needs the `reltest` profile.**

`[profile.release]` sets `panic = "abort"`, which the test harness cannot use,
and the settlement tests simulate days at a time, which is far too slow
unoptimized. `bun run test` already passes `--profile reltest`; a bare
`cargo test` in `rust/` is the wrong command.

**4. Playwright deadlocks on overlays that close on an outside press.**

The view dropdown in the top bar (and anything else floating over the page)
closes on a document-level `pointerdown` outside itself. A person never gets
stuck on it; the driver does: its actionability check sees the floating body
intercepting the click point and retries forever, without ever dispatching the
press that would have closed it. The failure reads as an endless
`element is not visible` / `subtree intercepts pointer events` retry loop on
some unrelated button. In `tools/uicheck.js`, any block that opens the
dropdown must close it again itself (`openView` / `closeView`); never rely on
the next click closing it implicitly.

## What each check is for

- **`bun run test`** - the tests in `rust/tests`: project file format, undo
  shape, the search index, settlement invariants, and the save round trip.
- **`bun run check`** - grows the lab world headless and writes `world.ppm`.
  Reports plant counts per species and whether everything rasterized.
- **`bun run check:civ -- [days] [out.ppm] [detail]`** - founds a settlement,
  runs it, verifies the bookkeeping (stock, deeds, paths out of every ring,
  bonds) and writes a picture. `GROW_SEED`, `GROW_COLS` and `GROW_ROWS` are
  read from the environment, which is how to sweep seeds or check a large map.
- **`bun run check:menu`** - rebuilds the menu search index from the live page
  and, with `--check`, fails if the committed index is out of date. Any change
  to a panel control or to the chrome in `index.html` needs this. Regenerate
  with `bun run index:menu`.
- **`bun run check:ui`** - clicks through both modes and every tab, exercises
  search, drag, undo and the settlement, and fails on any console error.
  Screenshots land in `/tmp/grow-shots`, which is the fastest way to see what
  a layout change actually did.

## Determinism

One seed is one world. Two runs of the same seed have to produce the same
pixels, and that is the acceptance test behind most of this:

```sh
cd rust
cargo run --profile reltest --bin civsmoke -- 60 /tmp/a.ppm
cargo run --profile reltest --bin civsmoke -- 60 /tmp/b.ppm
cmp /tmp/a.ppm /tmp/b.ppm
```

If a change makes those differ, it reordered an RNG draw somewhere. The same
comparison is what `tests/settlement_save.rs` does across a save and reload.

## The settlement save

The running settlement is written to `localStorage` under
`grow.settlement.v1`, separately from the project (`grow.project.v1`). To test
reload behavior by hand: open the page, enter Settlement, let it run, reload,
and the note beside the title should read `<town>, day <n>` rather than
`<town> founded`. Clearing it is `localStorage.removeItem('grow.settlement.v1')`
in the console, or the Reset all button, which clears everything.
