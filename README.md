# grow

Two halves of one project, in two modes.

**Plant lab** is a tool for authoring pixel art plants: drawable sampling boxes
per material, a shared shading curve, per species growth and spread parameters,
and a grid based world to test them in.

**Settlement** drops five settlers into a procedurally generated map grown from
those same species, and simulates what happens next: they forage, fell trees,
quarry stone, carry every plank to every building site, raise houses and
workshops, have children, trade, and work their way up a technology tree. Every
number behind it is a parameter you can change while it runs.

## Run

```sh
bun run dev        # http://localhost:5173
```

ES modules need HTTP, so open the served URL rather than the file directly.

## Modes

The two buttons above the panel tabs switch modes (or press `m`). Each mode has
its own tabs, its own toolbar and its own simulation; both read the same
materials and species, so anything drawn in the lab shows up in the settlement.

## Plant lab: the four panels

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
  rate and distance at which existing plants seed new ones. Offspring land
  anywhere on the ring around the parent, in any direction across the area.
* **Form and branching** - width and taper, branch chance, interval, angle
  range and depth, wander, phototropism (pull back toward vertical) and droop.
* **Leaves** - first depth that grows leaves, density, size range, the stem to
  leaf length, and whether leaf edge pixels get their own material.
* **Climbing and wrapping** - vines look for the nearest woody neighbor
  anywhere in the surrounding area and coil up it; the wrap pitch and sway set
  the coil, and the back half of each coil is darkened. With nothing to climb
  they creep sideways.
* **Limits** - footprint radius, height and tip count, each clamped by the size
  class ceiling set in the World panel.
* **Shading** - tone steps, core depth per material group, jitter, and whether
  core depth adapts to each shape (on lets thin twigs reach the darkest tone,
  off keeps them light).

### World

Area size in cells, the cell width and cell depth that set the viewing angle,
the sky band height, distance haze, ground shadows, sky colors and soil
texture; the size class ceilings; and the simulation settings (seed, ticks per
second, redraws per frame). Changing area dimensions restarts the run.

A cell depth equal to the cell width gives a straight top down grid; smaller
values tilt the plane toward the viewer. The sky band is the room above the far
row that tall plants grow into.

## The grid system

The world is a 2.5D area: a ground plane seen at an angle. Columns run left to
right (x) and rows run from the far edge to the near edge (depth). A cell is
drawn `cell width` wide and `cell depth` tall, so a row of depth is
foreshortened, and plants stand up out of their cell:

```
screen x = col * cellPx
screen y = skyPx + row * depthPx      (row 0 is the far edge)
```

Plants are composited back to front, so a plant in a nearer row overlaps one
behind it. Ground cover is drawn as a foreshortened disc lying on the plane,
everything else stands on it and casts a small contact shadow. Far rows are
lifted toward the light end of their own ramp by **distance haze**, which keeps
atmospheric depth inside the palette instead of tinting sprites out of it.

Occupancy is tracked per size class layer, one item per cell per layer:

| layer | size class   |
| ----- | ------------ |
| 0     | ground cover |
| 1     | herb         |
| 2     | shrub        |
| 3     | tree         |
| 4     | vine         |

So ground cover and a tree can share a cell, two trees cannot. A plant claims a
disc of cells around its own cell, as wide as its footprint radius, and asks
for a larger one as it grows. A refused request marks it confined: its tips
steer back inward instead of pushing into a neighbor, so a crowded plant grows
tall and narrow. Height is not a grid cost, only the footprint is. The
**Occupancy** toggle in the test window colors the claimed cells per layer.

## Settlement: the five panels

Entering the mode for the first time grows a wilderness (a few hundred
simulated seconds of the plant sim), scatters deposits, picks a spot and puts
five settlers next to a storehouse.

### Land

The map and the terrain generator: size, cell size, seed, noise scale and
roughness, water and rock levels, moisture and fertility, and how lush the
wilderness is. Deposits of stone, clay and ore are scattered per resource with
their own density, cluster size and richness; each holds a finite amount, so a
settlement that has emptied the ground near it has to reach further out.

The view section holds day and night, footpaths, chimney smoke, building
labels, and the water and path colors.

### People

Everything about a settler: walking speed, carry capacity, work rate, the share
of adults kept free to haul and build, the length of a day and the hours worked
in it, hunger and rest and healing, how fast people age, when they become
adults, how long they live, and how often couples have children. Work rates for
harvesting, mining, building, crafting and farming are here too, along with a
live roster of who is doing what, with hunger and health bars.

### Build

The planner's parameters (how many sites at once, spacing, sprawl, cost and
work scales, housing headroom, per category weights and how many people justify
another building of a kind), what is currently under construction and what it
is waiting for, and the full catalog. Every entry shows its cost, what it does
and whether the technology for it is known; **Build** places a site by hand.

Placing a site does not build it. The materials have to be carried there first.

### Economy

The store with every resource, its target stock, its price and its flow per
day; the treasury, net worth and storage used; a plot of population, food, coin
and buildings over the run; and the parameters behind prices, wages and
caravans.

Nothing sets a price directly. Each resource has a target stock that grows with
the population, and its price is the base price scaled by how far the store is
from that target, smoothed over time. Wages are only paid once a market stands,
which is also what brings caravans: they buy whatever the settlement has too
much of and sell it what it is short of.

### Tech

Research rates, whether research picks its own target, and the tree. A tech
costs points, needs its prerequisites, and pays out by unlocking buildings and
raising named modifiers (gathering speed, carry capacity, farm yield, and so
on). Points come from scholars in a school plus a small trickle from the
population. Pick any available tech to make it the target.

## How a settlement works

* **Everything is carried.** A woodcutter walks to a tree, fells it, and can
  carry one load home; the rest of the timber lies where it fell until someone
  comes back for it, and rots if nobody does. A building site accumulates the
  materials people bring it and only then can be raised.
* **Foraging is renewable, felling is not.** Ground cover is cut back to a
  third and grows again at whatever rate its species has in the lab, so the
  food supply is tied to the plants you authored. Trees and shrubs are felled
  outright and have to reseed.
* **Labor is reallocated every day.** Workplaces are ranked by what the store
  is short of, and by whether they still have anything to work: a forager camp
  with nothing left to cut within reach loses its priority, which is what
  pushes a settlement off foraging and onto farms.
* **The store is finite.** Deliveries that do not fit are left outside, and
  nobody carries home a resource the settlement is already drowning in.
* **Population follows food and beds.** Births need spare housing and food per
  person in store; people die of old age, of sickness (less often near a well)
  and of hunger.

## Test window

Play/pause (space), single step (`.`), fit (`f`), a speed multiplier up to 32x,
wheel to zoom, drag to pan, plus grid and occupancy overlays. The status bar
shows tick count, simulation time, plant counts per species, the redraw queue
and frame rate.

## Projects

State auto saves to localStorage; **Export** writes a JSON project and
**Import** loads one back. **New** resets to the defaults.

A project holds every parameter, including all of the settlement's, but not a
running settlement: reloading the page keeps the land and the rules and founds
it again.

## Checks

```sh
bun run tools/smoke.js out.ppm             # plant sim, grid invariants, PPM snapshot
bun run tools/civsmoke.js 60 town.ppm      # 60 days of settlement, bookkeeping, PPM snapshot
CHROMIUM_PATH=/path/to/chrome bun run tools/uicheck.js /tmp/shots
```

`civsmoke.js` founds a settlement, runs it for the given number of days and
checks the bookkeeping: no building on water or off its own footprint, no
worker a building does not agree it employs, no plant growing where a building
stands, no negative or over reserved stock, nobody walked off the map.

`uicheck.js` loads the page in headless Chromium, exercises both modes and
every tab, paints into a sampling box, resizes the world, queues a building,
and fails on any console error.

## Layout

| path              | purpose                                          |
| ----------------- | ------------------------------------------------ |
| `index.html`      | shell                                            |
| `styles.css`      | theme and layout                                 |
| `src/*.js`        | plant simulation core (no DOM) plus rendering    |
| `src/civ/*.js`    | settlement: terrain, people, economy, tech, draw |
| `src/ui/*.js`     | panels and the pixel grid editor                 |
| `tools/*.js`      | headless checks                                  |
| `ARCHITECTURE.md` | module map, data model and pipeline diagrams     |
