# grow

A browser tool for authoring pixel art plants: drawable sampling boxes per
material, a shared shading curve, per species growth and spread parameters, and
a grid based world to test it all in.

## Run

```sh
bun run dev        # http://localhost:5173
```

ES modules need HTTP, so open the served URL rather than the file directly.

## The four panels

### Materials

Sampling boxes are small pixel grids that plants take their colors from. There
is one per material by default (ground cover, soil, trunk, branches, leaf
texture, leaf edges, stem to leaf) and boxes can be added or removed.

* **Grid layout** switches between a separate box per material and one shared
  grid where each material owns a rectangular region. Switching to the shared
  grid copies the boxes into it; the two sync buttons copy in either direction
  at any time.
* Pencil, eraser, fill and pick; right click erases; mirror X paints both sides.
* **Make ramp** fills the box with a gradient between two colors, **Clear**
  empties it. In shared grid mode both act on the selected region only.
* The strip under the editor is the *resolved ramp*: the unique colors of that
  box sorted dark to light. Shading indexes into this, so a box with six tones
  gives six tone steps regardless of how they are arranged in the grid.

### Shading

One curve is shared by every plant. Each pixel gets a tone from two
measurements taken inside its own shape:

* **depth** - how far inside the silhouette the pixel is (0 edge, 1 core)
* **vert** - where it sits vertically inside that shape (0 top, 1 bottom)

```
tone = mid - centerDark * C(depth) + topLight * C(1 - vert) - bottomDark * C(vert)
```

`C` is a smoothstep between **curve start** and **curve end**, raised to
**gamma**. Pulling start and end together leaves a wide plateau, so the body of
an object stays a single flat color and only the rim shades - the "flat body"
preset does exactly that. The dotted line on the plot is the resulting tone
across a slice from edge to core.

Shapes are grouped before shading: trunk, branch and stem shade as one body,
leaf and leaf edge as another, so a leaf is shaded as a leaf and not as part of
the branch it hangs from.

### Species

Every parameter of a species, with an isolated growth preview above the form.
Growth rate, segment length, spread distance and leaf size are ranges; each
instance draws its own value from the range when it spawns.

* **Spawn and spread** - spawn rate, instance cap, minimum spacing, and the
  rate and distance at which existing plants seed new ones.
* **Form and branching** - width and taper, branch chance, interval, angle
  range and depth, wander, phototropism (pull back toward vertical) and droop.
* **Leaves** - first depth that grows leaves, density, size range, the stem to
  leaf length, and whether leaf edge pixels get their own material.
* **Climbing and wrapping** - vines look for a woody neighbor within the search
  radius and coil up it; the wrap pitch and sway set the coil, and the back
  half of each coil is darkened. With nothing to climb they creep sideways.
* **Limits** - footprint radius, height and tip count, each clamped by the size
  class ceiling set in the World panel.
* **Shading** - tone steps, core depth per material group, jitter, and whether
  core depth adapts to each shape (on lets thin twigs reach the darkest tone,
  off keeps them light).

### World

Grid size, cell size, the soil row, sky colors and soil texture; the size class
ceilings; and the simulation settings (seed, ticks per second, redraws per
frame). Changing grid dimensions restarts the run.

## The grid system

The world is a side view. Columns run left to right, rows top to bottom, and
rows from the soil row down are soil. Occupancy is tracked per size class
layer, one item per cell per layer:

| layer | size class   |
| ----- | ------------ |
| 0     | ground cover |
| 1     | herb         |
| 2     | shrub        |
| 3     | tree         |
| 4     | vine         |

So ground cover and a tree can share a cell, two trees cannot. A plant claims a
rectangle of cells as wide as its footprint radius and as tall as its height,
and asks for more as it grows. A refused request marks it confined: its tips
steer back inward instead of pushing into a neighbor. The **Occupancy** toggle
in the test window colors the claimed cells per layer.

## Test window

Play/pause (space), single step (`.`), fit (`f`), a speed multiplier up to 32x,
wheel to zoom, drag to pan, plus grid and occupancy overlays. The status bar
shows tick count, simulation time, plant counts per species, the redraw queue
and frame rate.

## Projects

State auto saves to localStorage; **Export** writes a JSON project and
**Import** loads one back. **New** resets to the defaults.

## Checks

```sh
bun run tools/smoke.js out.ppm     # headless sim run, grid invariants, PPM snapshot
CHROMIUM_PATH=/path/to/chrome bun run tools/uicheck.js /tmp/shots
```

`uicheck.js` loads the page in headless Chromium, exercises every tab, paints
into a sampling box, resizes the world, and fails on any console error.

## Layout

| path             | purpose                                          |
| ---------------- | ------------------------------------------------ |
| `index.html`     | shell                                            |
| `styles.css`     | theme and layout                                 |
| `src/*.js`       | simulation core (no DOM) plus rendering          |
| `src/ui/*.js`    | panels and the pixel grid editor                 |
| `tools/*.js`     | headless checks                                  |
| `ARCHITECTURE.md`| module map, data model and pipeline diagrams     |
