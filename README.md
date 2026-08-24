# grow

Two halves of one project, in two modes.

**Plant lab** is a tool for authoring pixel art plants: drawable sampling boxes
per material, a shared shading curve, per species growth and spread parameters,
and a grid based world to test them in.

**Settlement** drops five settlers into a procedurally generated map grown from
those same species, and simulates what happens next: they forage, fell trees,
quarry stone, carry every plank to every building site, raise houses and
workshops, marry, have children, trade, and work their way up a technology tree.
Rivers run across the map; boats run along them between towns; a settler who has
saved enough has their own hut pulled down and rebuilt as a house, then a manor,
then a tower. A town big enough rings itself with a wall and cuts the gates
where the paths already run. A settler with coin to spare opens a stall and
sells over the counter to their neighbors. Everybody keeps track of everybody
they have met, and what they make of them decides who they marry, whose counter
they buy from and how content they are. Every number behind it is a parameter
you can change while it runs.

The whole application is Rust compiled to WebAssembly. The page loads one
module and hands control to it; there is no other script.

## Run

```sh
bun run dev        # builds the wasm bundle, then serves http://localhost:5173
```

Needs a Rust toolchain with the `wasm32-unknown-unknown` target and
`wasm-bindgen-cli`:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
```

`bun run build` alone produces `pkg/`, which is what the page imports.
WebAssembly cannot be loaded over `file://`, so open the served URL rather than
the file directly.

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
simulated seconds of the plant sim), cuts the rivers, scatters deposits, picks a
spot and puts five settlers next to a storehouse.

One map holds several towns. A colony is a set of books over the shared map: its
own store, treasury, prices and research. When a town gets crowded and has the
supplies to spare, a party of its most restless adults, and the families that
follow them, walks off and founds another one somewhere far enough away to be
its own place. The panels that show one town's books start with a row of chips
to pick which.

### Land

The map and the terrain generator: size, cell size, seed, noise scale and
roughness, water and rock levels, moisture and fertility, and how lush the
wilderness is. Deposits of stone, clay and ore are scattered per resource with
their own density, cluster size and richness; each holds a finite amount, so a
town that has emptied the ground near it has to reach further out.

**Rivers** are cut after the noise rather than sampled out of it, so a river is
a path rather than a shape: a spring high up, then downhill until it reaches
standing water or the edge of the map. The channel widens downstream, the banks
either side are left damp and fertile, and the current is drawn along the flow.
A course that peters out in a hollow or runs too short is thrown away. Springs
are set per ten thousand cells, so a larger map gets more rivers rather than the
same few stretched across it.

The view section holds day and night, footpaths, chimney smoke, boats, current,
building labels, the water and path colors, and the two drawing controls: whether
to draw only what is on screen, and the zoom below which detail starts being
shed.

### People

The register of everyone who has ever lived here, and the parameters behind
them. Pick a name and the panel opens that settler's record: their town, their
parents, who they married and their children, the house they hold the deed to,
what they are doing right now, their purse, their personality, the trades they
have picked up and the log of what has happened to them. Sort the list by age,
coin, standing or name, and include the dead to read back through the families.

Below it: walking speed, carry capacity, work rate, the share of adults kept
free to haul and build, the length of a day and the hours worked in it, hunger
and rest and healing, how fast people age, when they become adults and marry,
how long they live, how often couples have children, what a settler keeps of a
wage, what a night at an inn costs, and the work rates for harvesting, mining,
building, crafting and farming.

Every settler is born with a personality that is fixed for life and inherited,
loosely, from their parents. Diligence sets how fast they work and learn, thrift
how much of a wage they keep and how soon they rebuild their house, curiosity
what they are worth at a desk, hardiness their resistance to sickness and
hunger, sociability whether they marry, and wanderlust whether they leave with
an expedition.

**Company.** Everyone a settler has stood near for long enough keeps a slot in
their memory, and the record shows the strongest of them: married, kin, friend,
rival or simply known, with how warmly on each. What two people make of each
other follows from how alike their temperaments are, plus a draw that belongs to
the pair - so some people never take to each other however alike they look on
paper. Family is filed at a birth and at a wedding and is never forgotten to
make room for a stranger; everybody else is, once the memory is full.

Affinity decides who somebody marries from among the matches of a like age,
whose stall they walk to, and how content they are - friends nearby against
rivals. The section under the register sets how often the sim looks at who is
near whom, how close counts, how many people a settler carries, how fast a bond
warms, and where the friendship and feud lines sit.

### Build

The planner's parameters (how many sites at once, spacing, sprawl, cost and
work scales, housing headroom, per category weights and how many people justify
another building of a kind), the rules for home upgrades and expeditions, the
towns on the map, what is currently under construction and what it is waiting
for, and the full catalog. Every entry shows its cost, what it does and whether
the technology for it is known; **Build** places a site by hand, for the
selected town.

Placing a site does not build it. The materials have to be carried there first.

**Walls and gates** have a section of their own: whether towns wall themselves,
the head count at which it becomes worth the timber, how far the ring stands
clear of the buildings, how many ways through it wants, and how many pieces may
go up at once. Wall pieces are counted apart from the sites above, or a ring
would stop the town building anything else. Fortification unlocks the palisade
and the gate; masonry adds a rampart of coursed stone, which goes up on the same
ring wherever the palisade has not reached.

**Stalls** has the price a keeper puts up, the margin they add over the town's
price, how many customers a counter needs to be worth keeping, and the most a
town will support. **Counters** below it lists every stall standing, who keeps
it, what is on it and what they are asking.

### Economy

One town's store with every resource, its target stock, its price and its flow
per day; the treasury, what is in settlers' purses, net worth and storage used;
a plot of population, food, coin and buildings over the run; and the parameters
behind prices, wages, boats and caravans.

Nothing sets a price directly. Each resource has a target stock that grows with
the population, and its price is the base price scaled by how far that town's
store is from that target, smoothed over time. Wages are only paid once a market
stands, which is also what brings caravans: they buy whatever the town has too
much of and sell it what it is short of.

Two towns on the same map are short of different things at the same time, which
is what gives the boats something to carry.

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
* **Population follows food and beds.** Only couples have children, and births
  need spare housing and food per person in store; people die of old age, of
  sickness (less often near a well, and less often if they are hardy) and of
  hunger.
* **Houses are owned by people, not by towns.** The first adult under an
  unowned roof takes the deed and keeps it for life; it passes to the oldest
  adult still under that roof when they die. A settler with enough saved coin
  has their own house pulled down and rebuilt one rung larger - hut, house,
  manor, tower - paying the price into the treasury, which is what then pays the
  laborers who carry the brick. Nobody plans a tower.
* **Anyone with a roof sleeps under it.** They walk to the door, step inside and
  stop being drawn; the windows light up instead, and they rest faster than
  somebody on a doorstep. Anyone without one takes a room at an inn if there is
  one free and they have the coin, and sleeps rough if not - which is what makes
  an inn worth building during a run of house rebuilds.
* **Towns grow out of towns.** A crowded, well stocked colony sends settlers out
  to found another, carrying supplies, a share of the treasury and everything
  the parent had learned. From then on they research separately and run short of
  different things.
* **Rivers are roads.** A colony with a dock builds boats there and sends them
  to the towns that want what it has too much of. A boat sells into the far
  town's store at the far town's prices and comes home with what this one is
  short of, which levels the two without either of them deciding to.
* **A wall is a ring, and a ring has gates.** A town big enough to be worth
  walling rings everything it has built. Gates go on the cells the town has
  already worn into paths and are kept apart from each other; wall goes on the
  ground nobody crosses. No piece is ever raised that would leave the outside no
  way in, so the ring tightens around its gates and stops at the last gap if
  there are none - and a finished gate is walkable, which is what then frees the
  stretches beside it to be closed.
* **A stall is one person's business.** Nobody plans one and nobody is assigned
  to keep one. A settler with coin to spare pays for the counter, stocks it out
  of the town store at the town's price with their own coin, and sells over it
  at a margin they keep - larger the more practised they are. It is the only
  thing that moves coin from one settler to another with the treasury nowhere in
  it, and the only use anybody has for coin besides a roof and a meal. Only what
  the town has spare is ever bought for a counter.
* **Everybody knows somebody.** Standing near each other is how settlers meet,
  and what they make of each other follows from how alike they are. It decides
  who they marry - among people of a like age, never across a generation - whose
  counter they walk to, and how content they are.

## Test window

Play/pause (space), single step (`.`), fit (`f`), a speed multiplier up to 32x,
wheel to zoom, drag to pan, plus grid and occupancy overlays. The status bar
shows tick count, simulation time, plant counts per species, the redraw queue
and frame rate; in the settlement it shows the day and hour, the towns, the
population, what is built, the stores, the fleet and the current drawing detail.

### Large maps

The map goes up to 512 by 256 cells. What that costs is memory for the pixel
buffers and time for the wilderness warmup, not frame rate: only the rectangle
the camera can see is ever composited or uploaded, and detail is shed in stages
as the zoom pulls back. At the closest zoom everything is drawn; a step out drops
the smoke, the carried loads, the lit windows and the ground shadows; another
turns plants into single dabs of their own average color and people into two
pixels; the furthest leaves the shape of the towns and the texture of the
forest. The threshold is a slider in the Land panel, so it can be pushed either
way.

## Projects

State auto saves to localStorage; **Export** writes a JSON project and
**Import** loads one back. **New** resets to the defaults.

A project holds every parameter, including all of the settlement's, but not a
running settlement: reloading the page keeps the land and the rules and founds
it again.

## Checks

Everything below `app` and `ui` is plain Rust with no browser dependency, so
the simulation runs headless.

```sh
bun run test                               # unit tests: determinism, invariants, project format
bun run check       out.ppm                # plant sim, grid invariants, PPM snapshot
bun run check:civ   60 town.ppm            # 60 days of settlement, bookkeeping, PPM snapshot
bun run check:civ   60 town.ppm coarse     # the same, drawn at a zoomed out detail level
GROW_SEED=909       bun run check:civ 200 town.ppm
CHROMIUM_PATH=/path/to/chrome bun run check:ui /tmp/shots
```

`civsmoke` founds a settlement, runs it for the given number of days and checks
the bookkeeping: no building on water or off its own footprint, no worker a
building does not agree it employs, no deed the owner does not agree they hold,
no counter its keeper does not agree they keep, no plant growing where a
building stands, no negative or over reserved stock, no boat aground, nobody
belonging to a town that does not exist, nobody walked off the map, nobody
walled out of their own town, and no settler remembering more people than they
can. It reports the towns, the rivers, the fleet, the ladder of homes, the
walls, the counters, the friendships and who is currently the richest settler
alive.

`GROW_SEED`, `GROW_COLS` and `GROW_ROWS` override the world it runs on. A
settlement is chaotic enough that one run says nothing about a change, so
judging one means sweeping a spread of seeds and comparing the distributions,
not reading a single number.

`uicheck.js` loads the page in headless Chromium, exercises both modes and
every tab, paints into a sampling box, resizes the world, queues a building,
and fails on any console error.

## Layout

| path                | purpose                                          |
| ------------------- | ------------------------------------------------ |
| `index.html`        | shell; imports the wasm module and nothing else   |
| `styles.css`        | theme and layout                                 |
| `rust/src/*.rs`     | plant simulation core (no browser) plus the shell |
| `rust/src/civ/*.rs` | settlement: terrain, towns, people, boats, draw   |
| `rust/src/ui/*.rs`  | panels and the pixel grid editor                  |
| `rust/src/bin/*.rs` | headless smoke checks                             |
| `rust/tests/*.rs`   | determinism, grid invariants, project format      |
| `tools/uicheck.js`  | headless browser pass over every panel            |
| `pkg/`              | build output: the wasm module and its loader      |
| `ARCHITECTURE.md`   | module map, data model and pipeline diagrams      |
