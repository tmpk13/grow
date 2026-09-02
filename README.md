# grow

Two halves of one project, in two modes.

**Plant lab** is a tool for authoring pixel art plants: drawable sampling boxes
per material, a shared shading curve, per species growth and spread parameters,
and a grid based world to test them in.

**Settlement** drops five people into a procedurally generated map grown from
those same species, and simulates what happens next: they forage, fell trees,
quarry stone, carry every plank to every building site, raise houses and
workshops, marry, have children, trade, and work their way up a technology tree.
Rivers run across the map; boats run along them between towns; a person who has
saved enough has their own hut pulled down and rebuilt as a house, then a manor,
then a tower. A town big enough rings itself with a wall and cuts the gates
where the paths already run. A person with coin to spare opens a stall and
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

The tool opens on the settlement, because that is the thing that runs; the lab
and the sprite editor are where what it is made of gets authored.

The three buttons above the panel tabs switch modes (or press `m` to go round
them). Each has its own tabs, its own toolbar and its own stage; all three read
the same project, so anything drawn in the lab or the sprite editor shows up in
the settlement.

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
* **Undo** and **Redo** live in the top bar (ctrl+z, ctrl+shift+z) and cover the
  whole project, not just the editors: a stroke, a layer, a species parameter, a
  world size, anything a panel can set. A slider held through a range of values
  is one step back rather than one per value.
* **Brush color** is a plain color box, and **Wheel** opens an HSV wheel beside
  it: hue around, saturation out from the middle, value on the slider under it,
  and a hex field for a color you already know.
* **Make ramp** fills the box with a gradient between two colors, **Clear**
  empties it. In shared grid mode both act on the selected region only.
* The strip under the editor is what the box will read as, one row per height
  of the thing drawn from it, top of the box at the top. Two things about the
  box reach the object: **how much of it** a color covers decides how much of
  the shading it holds, so a box that is mostly one green shades mostly that
  green and a highlight drawn as two pixels stays a highlight; and **where in it**
  a color was drawn decides how far up the object it appears, so the top of the
  box draws the top of the object and never its foot. A box whose rows are all
  alike reads the same all the way down. The swatches above the strip are the
  palette, one entry per color however little of the box it holds.

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

## Sprite editor

A pixel editor for animations, and the other way to draw a person. The sheet is
drawn on the stage rather than in the panel: left button draws, right erases,
middle button or a held ctrl drags, wheel or pinch zooms.

* A **sheet** is a frame size, a rate, and a stack of layers. The toolbar picks
  which one; the **Sheet** tab names, resizes, adds and removes them.
* **Layers** stack bottom to top; the row you pick is the one you draw on, the
  switch beside it hides one without throwing it away, and **Merge down** folds
  one into the layer beneath it. Up to eight.
* **Frames** run along the strip in the panel and are stepped through from the
  toolbar (or `.`). Add an empty one, duplicate the current one to nudge a pose
  rather than redraw it, or shuffle one left and right. Up to twenty four.
* **Onion** shows the frame before this one faintly behind it, and **Play** runs
  the sheet at its own rate on the stage.
* **Drop images** onto the zone in the panel and they land on the selected
  layer, scaled down to fit the frame and centered. Several at once fill
  successive frames, one each, starting from the frame being drawn - so a
  reference can go on a layer of its own and be drawn over on the one above.
* **Nudge** shifts the art by a pixel: the selected layer in the selected frame,
  or the whole sheet with the switch beside the buttons.
* **Use as person art** sends the sheet to one of the five person motions. It
  is copied rather than followed, so the town does not change under you while
  you keep drawing. The motion's card in the settlement's People panel says
  which sheet it came from and offers to take that sheet again, which is how a
  change is pushed.
* **Download PNG** saves the sheet as one image, every frame side by side at one
  pixel each, which is the shape a drop zone reads a sheet back in.
* **Kept sheets** are copies held outside the project, so art outlives the
  project it was drawn in. Every save adds to them while the switch is on;
  **Restore** brings one back into the project and **Delete** removes it for
  good, after asking, because undo does not reach outside the project.
* Resizing a sheet crops or pads it. Pixel art does not survive resampling, so
  the art keeps its place and the new room is empty.

### The map page

With the settlement's **Experimental** switch on, the sprite editor grows a
third page: the settlement's own map, drawn by hand instead of grown from
noise.

It is the same pixel editor. The stage is one pixel per map cell, the tools are
the ones already in the toolbar - pencil, fill, eraser, pick, line, mirror -
and the map changes under the pointer: there is no draft and nothing to apply.
What the page adds is that the colors mean something. The palette is a legend
of what land is, and it covers three different questions:

* **The ground** - water, rock, a rock face, grass, sand - is the map itself. A
  cell somebody has already built on keeps what it has.
* **A zone** - trees only, low growth only, nothing grows - says what may take
  root there, and is drawn nowhere but on this page. The settlement never shows
  one: it is a thing about what a cell will become rather than about how it
  looks. The eraser takes one off again.
* **Sky** is not on the map at all. It is a mark on this page, kept only while
  the page is open, saying which part of the picture underneath is sky.

A stroke can be taken back with undo, the same button and the same keys as
everywhere else, though these strokes are not part of the project's own history
- the map is not in the project - and the last two dozen of them are what is
kept.

* **Drop a picture** and it is laid under the map, corner to corner, to trace
  over. It is never part of the project and never part of a settlement: the
  picture goes when the page does, and what is kept is the map painted with it
  there.
* **How strongly it shows** is how much of the picture reads through the map
  drawn over it. All the way down is the map on its own; all the way up takes
  the map off and leaves the picture, which is what tracing a coastline wants.
  Zones and sky marks stay legible either way.
* **Picture pixels to a cell** is how much of the picture goes to one cell. It
  is guessed when the picture arrives - art drawn eight screen pixels to a pixel
  comes back as eight - and it is what decides how large a map the picture makes.
* **Use it as the map** reads the whole picture in: every cell becomes the
  nearest thing in the legend, the map takes the picture's own size at that
  scale, and the settlement is founded again on it. There is no ceiling on the
  size. A very large map costs memory and a long wilderness warmup, and the
  panel says so rather than refusing.
* **Take the sky colors** reads the top and the bottom of whatever is marked
  sky out of the picture and sets the world's sky gradient to them.
* **Fill by color in the picture** turns the fill tool into a magic wand over
  the picture rather than over the map: press on the sea in a photograph and
  every cell whose color stays near enough to that one, spreading out from
  where you pressed, becomes whatever the legend has selected. **How near the
  color** is how far "near enough" goes - 0 takes that exact color only, 1
  takes the whole map whatever it looks like.
* **Wipe the map to ...** turns every cell into the ground the legend has
  selected and takes every zone and sky mark off with it: a blank sheet to draw
  on. Wipe to water and draw the land in, or wipe to grass and draw the sea.
  The button says which it would use, and it is one step back like any other
  stroke.
* **Take every zone off** clears the zones and leaves the ground alone.

**Rock face** is the one kind of ground that is new. Water is crossed by
swimming and rock is walked on and built on; a face of rock is neither. Nobody
crosses one, nothing takes root in one, and nothing is ever built on one. The
terrain generator never makes one, so a cliff on a map is always somewhere
somebody drew it - by hand here, or from a picture with the Land panel's
**Zones from a picture**.

### Pictures dropped in

Art drawn large - eight screen pixels to a pixel, or sixteen - is read back
down to the pixels it was drawn in on the way in, so a sheet holds what was
drawn rather than a magnified copy of it.

**Picture pixels to a pixel** on the Sheet tab is the default for every drop
target in the tool. Zero works it out from the picture, one takes it exactly as
it is, and anything else is taken as given. Each drop target - the sheet, a
person's motions, the things people make - carries the same number beside it
and can be set to something of its own, since people are drawn at one size and
the things they build at another. It is kept with the browser rather than with
the project.

### Selecting part of a cel

**Marquee (M)** drags a rectangle out on the stage instead of drawing. With one
up, the nudges move what is inside it rather than the whole cel, and **Clear
inside** empties it; the panel says how large it is and where. What moves out of
the rectangle is dropped, so a selection is a window on the cel rather than
something that smears its contents across it. Escape or **Drop selection**
lets it go.

### Getting art out

**Download PNG** on the Sheet tab writes the whole strip, one image pixel per
art pixel, which is the shape a drop zone reads a sheet back in. **Download
this frame** writes only the frame showing. **Download zip** writes every
ticked sheet at once, a folder per sheet, with a file per frame beside the
strip if that is asked for.

The archive is stored rather than compressed - a PNG is deflated already, so a
second pass would cost code and save nothing - and it is written here rather
than by a library. It carries no clock, so exporting the same sheets twice
gives the same bytes.

### Reordering

Frames and layers move by dragging as well as by stepping: pick one up and drop
it anywhere in the strip or the stack and it walks past the others rather than
swapping with the one it landed on, so dragging the first frame to the end
leaves the rest closed up in order. The frame or layer you dragged stays
selected. Both are one undo step.

### Keys

On anything with a keyboard the tool buttons carry their key - **Pick (P)** -
and the Draw panel has a folded **Keys** list with the rest of them: `B` pencil,
`E` eraser, `G` fill, `P` pick, `M` marquee, `X` mirror, `O` onion skin, space to play,
`,` and `.` for the frame before and after, `[` and `]` for the layer below and
above, `F` to fit the sheet to the stage. Nothing is listed on a phone, which
has no keys to press.

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

## Settlement: the six panels

Entering the mode for the first time grows a wilderness (a few hundred
simulated seconds of the plant sim), cuts the rivers, scatters deposits, picks a
spot and puts five people next to a storehouse.

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
building labels, the water and path colors, how long the map waits before taking
the whole window on its own, and the two drawing controls: whether to draw only
what is on screen, and the zoom below which detail starts being shed.

**Weather** is in there too: whether clouds pass at all, how much of the sky
they take, how fast they drift, how strongly their edges churn, and **Cloud
start height** - how far down the sky they begin, as a share of it. Zero fills
the sky to the top of the frame; raising it leaves clear air above the weather
and slides the whole band down toward the horizon. The line is the same one the
sky past the map's edge is drawn against, so with **Clouds past the map's edge**
on a shape carries across the boundary rather than stepping at it.

**Grow it instead** makes the map larger without starting the settlement over.
The new land goes on the right and along the bottom, so every column and row
that is already there keeps its number and nothing standing on one moves: the
town, its people, the plants and the loads on the ground carry on exactly where
they were. The new ground arrives with a wilderness on it, warmed for as long as
a fresh map is and with the old land held still while that runs, so making the
map bigger is not a week passing. Wild growth goes up with the area at the same
time, because the land carries a count of plants rather than a density and
without that the new ground would come out bare. Rivers and deposits are placed
in the new land only, and a course traced into the old map stops at the boundary
rather than cutting a channel through somebody's town.

### People

The register of everyone who has ever lived here, and the parameters behind
them. Pick a name and the panel opens that person's record: their town, their
parents, who they married and their children, the house they hold the deed to,
what they are doing right now, their purse, their personality, the trades they
have picked up and the log of what has happened to them. Sort the list by age,
coin, standing or name, and include the dead to read back through the families.

Below it: walking speed, carry capacity, work rate, the share of adults kept
free to haul and build, the length of a day and the hours worked in it, hunger
and rest and healing, how fast people age, when they become adults and marry,
how long they live, how often couples have children, what a person keeps of a
wage, what a night at an inn costs, and the work rates for harvesting, mining,
building, crafting and farming.

Every person is born with a personality that is fixed for life and inherited,
loosely, from their parents. Diligence sets how fast they work and learn, thrift
how much of a wage they keep and how soon they rebuild their house, curiosity
what they are worth at a desk, hardiness their resistance to sickness and
hunger, sociability whether they marry, and wanderlust whether they leave with
an expedition.

**Company.** Everyone a person has stood near for long enough keeps a slot in
their memory, and the record shows the strongest of them: married, kin, friend,
rival or simply known, with how warmly on each. What two people make of each
other follows from how alike their temperaments are, plus a draw that belongs to
the pair - so some people never take to each other however alike they look on
paper. Family is filed at a birth and at a wedding and is never forgotten to
make room for a stranger; everybody else is, once the memory is full.

Affinity decides who somebody marries from among the matches of a like age,
whose stall they walk to, and how content they are - friends nearby against
rivals. The section under the register sets how often the sim looks at who is
near whom, how close counts, how many people a person carries, how fast a bond
warms, and where the friendship and feud lines sit.

**Pictures for made things.** Buildings, walls, gates, stalls, boats and the
loads people carry are all drawn out of the sampling boxes unless there is a
picture for them. **Pictures for made things** in the Build panel has a slot
per thing per state: one per catalog entry, plus the boat and one per resource
in hand, each of them offering *Always*, *Going up*, *With somebody at it* and
*After dark* - a boat offers *Carrying cargo* instead, and a load in hand is
only ever itself. Drop images on a slot, or send a sheet to one from the sprite
editor with **Use for that**.

A thing with a picture for one state only is drawn from it in that state and
generated the rest of the time, the way a person motion with nothing on it
borrows from a related one. A building still going up is the exception in the
other direction: it never falls back to the finished picture, because one image
cannot say how far a wall has got.

A picture is scaled to the box the generator would have filled: as wide as the
footprint, as tall as the walls and roof over the depth of it, standing on the
front edge, so art and generated things stand together on the same map. **Draw
made things from pictures** turns the lot off without losing any of them.

That is a hundred and thirty slots, so the list is searched rather than shown.
The box over it runs the same ranking the menu search does, over its own index
and its own meaning table: with nothing typed only what has a picture is
listed, **Every slot** shows the rest, and **Meaning** finds the lamp post from
"lantern" and the inn from "tavern".

**Foliage over people.** A person walking behind a bush is behind it, which is
right and also makes them hard to follow through a wood. **Foliage over people**
in the Land panel's View section offers two other readings: *hatched* leaves
every other pixel of the covering foliage out, so the person shows through in
a screen pattern; *see through* mixes the foliage over them by a settable
amount. The mark rides in the alpha byte of the composite buffer rather than in
a mask beside it, so it costs nothing on a map that is already megabytes of
pixels.

**Taking a sheet again.** A clip keeps its own copy of the art, so a sheet
drawn on after a motion took it does not change the people on the map. Both
ends now say so: the motion's card in the People panel reads *from Person,
which has been drawn on since - take it again to catch up*, and the sprite
editor's own buttons read **Standing - taken** or **Standing - out of date**.
The sheet is fingerprinted when it is taken and compared with what it is now,
so drawing something and undoing it back leaves the clip current.

**Person sprites.** People are drawn from a generated body by default: three
pixels wide, a head, and a two frame walk. Drop images on the panel to replace
it. There is a slot per motion - standing, walking, carrying, working, sleeping
- and each keeps its own art, its own number of frames and its own playback.

Drop one image and it is read as a strip: a sheet whose width is a whole number
of its height is cut into that many square frames, and anything else arrives as
one frame you then set the count on. Drop several and each becomes a frame, in
the order their names sort, so `walk1.png walk2.png walk10.png` lands in that
order rather than the browser's. Frames of different sizes are centered on a
common box and stood on its floor, so they line up at the feet. Clicking a slot
opens a file picker instead, for a keyboard.

The sheet is kept whole rather than cut up, so the frame count stays editable
afterwards: a strip read as four frames becomes six by typing six. Per slot you
also set the drawn height in cells (width follows the shape of the frame), how
far the art is lifted off the ground, whether it mirrors when facing left, and
the rate.

The rate is either frames per second or frames per cell walked. Tie a walk to
steps and it never slides and never runs on the spot, because the same counter
that made the generated person take a step advances it; leave a sleep or an
idle on the clock, where standing still should still breathe.

A slot with nothing dropped on it borrows from a related one - carrying falls
back to walking, working and sleeping to standing - so one walk sheet is enough
to replace the person everywhere. A slot with nothing behind it at all falls
back to the generated body, and so does everything when the switch at the top of
the section is off, which hides the art without giving it up.

Frames are capped at 24, and one frame at 64 pixels a side; anything larger is
scaled down on the way in rather than refused. Sheets are saved with the project
like any other pixel buffer, so they travel through Export and Import and come
back on a reload - but they are the one thing in a project big enough to fill
the browser's storage, and the section says what they are costing.

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
per day; the treasury, what is in people' purses, net worth and storage used;
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

### Experimental

One switch, off by default, and everything under it. With it off nothing in
here is asked anything: no balloon is built, nothing is spent on one, and the
settlement is the settlement it has always been. It can be turned on while a
town runs and turned off again.

**The map editor** is the other thing this switch puts up, and it is not on
this panel: with it on, the sprite editor grows a third page. See
[Sprite editor](#sprite-editor) below.

**Hot air balloons.** A town with a school and cloth to spare sews a canopy,
burns charcoal under it and sends it up over itself. What can be seen from up
there is worth more to the scholars than another day at the bench, so research
runs faster for as long as one is in the air. It climbs, drifts on whatever wind
it caught, and comes down; the town waits out the interval before building
another. No school, no balloon. How much a canopy is worth, how many a town
keeps up, how long a flight lasts, how high it gets, how fast the wind carries
it and what it costs are all here, and the list below them says what is in the
air right now and where.

## How a settlement works

* **Everything is carried.** A woodcutter walks to a tree, fells it, and can
  carry one load home; the rest of the timber lies where it fell until someone
  comes back for it, and rots if nobody does. A building site accumulates the
  materials people bring it and only then can be raised.
* **A load is worth a walk, up to a point.** Nobody crosses the map for
  something lying on the ground: past the distance in the People panel they
  leave it, and one that was cut by hand is worth half again as long a walk
  because somebody asked for that one. The nearest load beyond anybody's reach
  is still kept as a last resort, below every other job there is, so a town
  with nothing nearer to do fetches it rather than standing about.
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
  adult still under that roof when they die. A person with enough saved coin
  has their own house pulled down and rebuilt one rung larger - hut, house,
  manor, tower - paying the price into the treasury, which is what then pays the
  laborers who carry the brick. Nobody plans a tower.
* **Anyone with a roof sleeps under it.** They walk to the door, step inside and
  stop being drawn; the windows light up instead, and they rest faster than
  somebody on a doorstep. Anyone without one takes a room at an inn if there is
  one free and they have the coin, and sleeps rough if not - which is what makes
  an inn worth building during a run of house rebuilds.
* **Towns grow out of towns.** A crowded, well stocked colony sends people out
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
  to keep one. A person with coin to spare pays for the counter, stocks it out
  of the town store at the town's price with their own coin, and sells over it
  at a margin they keep - larger the more practised they are. It is the only
  thing that moves coin from one person to another with the treasury nowhere in
  it, and the only use anybody has for coin besides a roof and a meal. Only what
  the town has spare is ever bought for a counter.
* **Lamp posts.** A post with a light on the head, which burns after dark and
  throws a pool of warm light over the ground around it. The town builds a few
  once there are enough people to want them, and they can be queued by hand from
  the Build panel like anything else. The light is added over the night tint
  rather than cut out of it, because a lamp gives light off.
* **A cut tree goes over.** Nothing with a stem in it vanishes out of the hand
  that cut it: it tips from its foot, over a second or so, and is only off the
  map once it is lying down. The ground it stood on is free from the moment of
  the cut, because something else may start growing there. Ground cover, which
  is cut back rather than pulled up, has nothing to tip.
* **Trees are in the way, and growth is slow going.** A shrub, tree or vine
  with enough of itself grown shuts the one cell its stem is in, so people walk
  round it rather than through it. Only the stem: a canopy is walked under, and
  a wood is walked through rather than round. Everything else standing is a
  price rather than a wall - pushing through a meadow costs more than crossing
  open ground, in proportion to how much is standing there, so people drift
  round a thicket when going round is not much further and cut straight through
  when it is. A route taken often enough wears into a path, and a worn path is
  cheap again: that is how a road ends up through the meadow rather than around
  it. The switch, how much of a plant is a wall, and what pushing through costs
  are all in the People panel; turning the cost to zero is how it used to be.
* **A house nobody lives in falls in.** Past a wait, an empty home loses its
  roof from the ridge outward and then its walls from the top down, weathering
  as it goes. When there is nothing left the ground comes back and a share of
  what it was built from is left lying in the rubble for anybody still around
  to fetch. Somebody moving in at any point puts it right again at the same
  rate it was going wrong, so a town between owners loses nothing - and a town
  that has died out does not stand forever.
* **Water is crossable, at a price.** A step into water costs the pathfinder
  several steps of dry ground, so a river is swum only when walking round it
  would be much further, and a swimmer moves at a fraction of walking speed and
  wears no path behind them. Both numbers are in the People panel. Somebody in
  the water is drawn cut off at the surface.
* **People can be picked up.** Turn on **Move people** above the map and a
  press on a person lifts them off it: the pointer carries them, and letting go
  puts them down where they were dropped, or on the nearest ground they can
  stand in if that was a roof or a cliff. Whatever they were doing is given up
  properly, so nothing is left reserved for a delivery nobody is coming to make,
  and they plan again from where they land. A press on empty ground still drags
  the map, as do the middle button and a held control key.
* **What you cut, they fetch.** Turn on **Harvest** above the map and a press
  on something growing starts cutting it: hold, or drag across a patch, and a
  bar over each plant fills as the work goes in. Let go too soon and the bar
  runs back out and leaves nothing. What a finished cut is worth is left lying
  where the plant stood, exactly as an overfull load would be, and a person
  fetches it before they go and find work of their own - a hungry town excepted,
  which fetches food first whatever was asked for. Everything that could be cut
  pulses faintly while the switch is on, and whatever is under the pointer
  pulses firmly.
* **They learn what you cut for.** Every cut is remembered against the species
  it was made on. Gatherers walk further for a species that has been cut for
  them and take a smaller specimen of it than they would otherwise have
  bothered with, so clearing a stand of one plant by hand turns the whole town
  toward it. What has been taught is listed under Learned by hand in the Tech
  panel.
* **Everybody knows somebody.** Standing near each other is how people meet,
  and what they make of each other follows from how alike they are. It decides
  who they marry - among people of a like age, never across a generation - whose
  counter they walk to, and how content they are.

## Test window

Play/pause (space), single step (`.`), fit (`f`), a speed multiplier up to 200x
on a logarithmic slider, wheel or pinch to zoom, drag to pan, plus grid and
occupancy overlays. In the settlement, Move people turns a press on the map into
picking a person up rather than dragging the view, and Harvest turns it into
cutting what is growing. The status bar
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

Zoomed out far enough that the whole map is on screen, the camera is drawing one
screen pixel per block of world pixels and discarding the rest, so the frame
stops producing them: the ground, the compositing and the upload all step over
the same grid of one pixel in that block. Nothing visible changes - at half zoom
the result is byte identical to uploading everything and letting the canvas
shrink it - and the whole map at 512 by 256 costs about a fifth of what it did.

## Projects

State auto saves to localStorage; **Export** writes a JSON project and
**Import** loads one back. **New** resets to the defaults.

A project holds every parameter, including all of the settlement's, but not a
running settlement. That is kept apart, under its own key in localStorage,
because it is not a document: it is a hundred people, their houses, their
histories and the wilderness they are cutting down, and a project file sent to
somebody else has no business carrying them. It is written down every twenty
seconds while it runs, and again when the page is closed or the tab is hidden,
so a reload picks the same town up on the same day rather than founding a new
one.

A saved settlement is only good for the world it grew on. Change the map size,
the terrain settings or the seed and it is thrown away rather than dropped onto
ground that no longer matches it - which is also what **New land**, **Rebuild
this land**, **New** and **Reset all** all do to it deliberately. A very large
map can outgrow what a browser will hold, and a settlement past three megabytes
of text is left unsaved and says so beside the title.

**Reset all** goes further than New: it empties every store the page has in this
browser - the saved project, the window settings, the session store, any cached
files and any indexed databases - and reloads, so what comes back is the tool as
it was the first time it was opened. It asks first, and it cannot be undone.

Kept sheets are the one thing it leaves standing. Art outlives the project it
was drawn in, which is the whole point of keeping it separately, and a button
for clearing a stuck page is not a reason to lose it; a kept sheet goes when it
is deleted from the sprite editor's Sheet tab.

### Fear of the dark

A town used to raise lamp posts the way it raised anything else: the planner
decided it wanted one. It does not any more. Walking home after dark with no
lamp in sight wears on a person - slowly, and less on a hardy one - and
daylight, a roof and a lit street all settle it again. What is left is a memory
of nights rather than a mood, and it is what makes somebody would rather spend
their coin on a lamp post outside where they sleep than keep it.

The price is the same for everybody, which is the point: the people most
afraid of the dark are the ones sleeping rough, and the ones who can act on it
are the ones with money. A lamp goes up because a well off person has had
enough of the walk home, and the fear it was raised against comes down with it
for everybody who passes under it. The Register shows how calm each person is,
and the smoke run reports the town's fear of the dark beside its lamps.

Turning **Start over if everyone dies** and the rest of the settlement's
switches aside, this is one of the three places a building is raised by a
person rather than by a plan - the others being a house rebuilt one rung larger
by its owner, and a camp fire.

### Camp fires

A lamp post is the answer for anybody who can afford one. A camp fire is what
is left after that: somebody further gone than the price of a post, out of
every light, stops walking, gathers what is lying around and lights a fire
where they stand. It costs the town nothing - it is deadfall, not timber out of
the store - it throws a small light and a plume of smoke while it lasts, and
then it burns down to nothing and is gone. Nothing is salvaged from one,
because it burned.

It is the only thing on the map that takes itself away again, and the machinery
behind that is general: any building type with a lifetime stands for that many
settlement seconds after it is finished and then comes down on its own. The
camp fire is the only one that has one.

The threshold sits **above** the one that buys a lamp, and it has to. A fire
lights the cell it stands on, which settles the fear of whoever is at it the
same way any other light does; put the fire first and nobody is ever frightened
enough to pay for a post, and the town never lights a street. For the same
reason a fire burning outside a house does not count as a lit street when the
town is deciding whether that house wants a lamp: it will be ash by tomorrow,
which is the whole reason somebody pays for a post instead.

**Somewhere to sit.** Whoever lights a fire sits down at it rather than walking
on, and anybody else out in the dark within walking distance comes over and
takes a place at the ring. The places are the cells around the fire, so a fire
lit in a corner holds fewer people than one lit in the open, and a place is
held only while somebody is in it. Sitting there settles the dark far faster
than standing under a lamp does, and that is the whole reason the walk is worth
taking: turn the warmth down to one and a fire is only a light again.

Nobody is made to go, and nobody stays longer than they need to. A person with
a bed and nothing much frightening them walks home in the first place, which is
what keeps the houses full; the ones who gather are the ones the night has got
to and the ones with no bed to walk to at all. Once the fire has settled the
dark, whoever has a bed goes to it and whoever has not stays by the light.
Sitting there is a doze rather than a night's sleep - worth about a third of a
bed - so a night out is still a night out. What
comes of it is not only warmth - the people sitting round a fire are standing
near each other, which is how the town's bonds are made, so a bad night out is
also where friendships and feuds come from.

Every number - whether people light them at all, how frightened is frightened
enough to light one, how long one burns, how many a town may have going at
once, how many can sit at one, how frightened is frightened enough to walk to
one, how far one is worth walking, and how warm it is - is in the Build panel.

### Farms and water

A farm's yield follows the fertility under its fields, and now also how wet
they are. Working a field dries it out. Damp ground within **Damp ground reach
(cells)** of a river or a lake fills it back up on its own, every field in
reach counting toward it, so a farm on a bank never runs dry and never asks
anyone for anything. A farm out in the dry sends whoever works it to the
nearest bank with a bucket, one at a time - three hands all walking to the
river would leave nobody working the field.

A parched field is poor rather than barren: **Yield with no water** is the
share it still brings in. The farming rate went up to match, so a well watered
farm is better than a farm used to be and a parched one is worse - siting a
farm by the water is now worth doing. The Build panel says how wet each farm's
fields are, where its water comes from, and what share of the yield that
works out to.

### Waiting on a rebuild

A setting the running world was built from - grid size, cell size, the terrain
knobs, the deposits - does not rebuild anything as it moves. A slider that
restarted a settlement at every value it passed through would be a slider
nobody could hold. The setting is starred, a bar above the panel says how many
are waiting, and **Apply** builds the world from them. **Discard** puts them
back the way the running world has them.

Leaving the panel with one waiting asks, with the three things it could mean:
apply and go, discard and go, or stay here. Undoing a change back to what the
running world was built from clears the star on its own.

### Dying back

Nothing on the map blinks out. A plant past its **Max age** dries out over
**Shrivel (s)** instead: it browns toward straw and comes apart from the tips
down, the thin end first, with a little noise per pixel so the edge stays
ragged. Only when there is nothing left is it taken off the map and its cells
handed back. A plant that is cut down is a different thing and goes at once,
because somebody carried it away.

The default is six seconds, which is fast enough to read as dying rather than
as a slow fade. The shrivel is re-drawn a dozen times from start to finish, not
once a frame, because re-rastering is the expensive part of the simulation and
a field can die at once.

### Starting over

Under **Founding party**, **Start over if everyone dies** brings the settlement
back after **Wait before starting over (s)** with nobody left alive. It is off
by default: a town dying out is usually the thing being watched for, and
clearing the evidence half a minute later would be no help. The wait runs on
settlement time, so a paused world never counts down and a fast one gets there
sooner.

## Finding a setting

There are eleven panels and a few hundred settings across them, so the top bar
has a search box. Press `/` from anywhere in the page, type a few letters, and
the list under it ranks every control in every panel of every mode, each with
the path to where it lives. Arrow keys move, enter goes; the tool switches
mode and tab for you, scrolls the control into view, flashes it and puts the
keyboard on it. Every word typed has to land somewhere on a match, so a second
word narrows rather than widens.

The index is not written by hand. Every labeled control the panels build gets a
stamp as it is built, and `tools/menuindex.js` walks the running page reading
them, so search cannot offer a control the build does not have. `bun run
check:menu` fails if the committed index has drifted from the page.

**Meaning** next to the box is off by default and matches on what a setting is
for rather than how it is spelled: "salary" finds **Pay wages**, "money" finds
the treasury, "colour" finds the brush color. Rows found that way are marked,
so an answer that no amount of squinting at the letters explains says where it
came from.

The switch is answering out of a table built ahead of time, not a model running
in the page. `tools/menu-terms` scores every word in a static embedding model's
vocabulary against every entry in the menu index and keeps the few entries each
word is closest to; only those answers ship. The model itself is thirty
megabytes and its crates want threads, native TLS and a filesystem, none of
which a page compiled to WebAssembly has, so it stays a build step. The table
is tied to the index it was built against and is ignored if the menus have
moved since, in which case the switch is simply not offered.

## Window

Two controls in the top bar belong to the browser rather than to the project,
and are remembered separately from it:

* **Text** scales every label, control and panel. Everything in the stylesheet
  is sized in `rem` or fractions, and the root size is itself relative to the
  browser's own font setting, so a reader who has raised that keeps the increase.
  The size is taken when the slider is let go rather than while it is dragged -
  what it resizes is the page the slider is in, so applying it live walks it out
  from under the pointer - and the box beside it takes a size in per cent for
  anyone who would rather say one than hunt for it.
* **Hide menu** folds the panel away and gives the map the whole window. What
  was in the middle of the view stays there.
* **Fold all** pulls every section of the showing panel one way and then offers
  the way back. Sections arrive folded - a panel is longer than a window and
  the map is what most of the window is for - so a tab opens as a list of
  headings and you pull open the one you came for. Which ones you have opened
  is remembered per browser, and a section that was open stays open when its
  panel is rebuilt under it.
* **Fullscreen** goes further: the top bar, the panel, the toolbar and the
  status line all go, leaving the world and one faint button in the corner to
  get back out. The browser is asked for the screen at the same time, so escape
  leaves too; if it refuses, the button and escape still work. The camera is
  pulled back to fill the space it gains, but never pushed in, so a view zoomed
  into one corner keeps its place and simply shows more around it.

A settlement left alone does the same thing on its own. With nobody touching
anything for **Fullscreen when idle** seconds - twenty by default, in the Land
panel's View section, zero for never - the chrome folds away and the map takes
the window. This is not the browser's fullscreen: a window nobody has touched
cannot ask for the screen, the request needs a gesture behind it and would be
refused. There is nothing to press to get out of it either, because anything at
all gets out of it: a moved pointer, a key, a touch. It waits for the
settlement only, and never while a question is on the screen.

### Taking a section with you

Every section of the menu that holds settings carries two small buttons in its
head. **Copy** puts that section on the clipboard as text; **Save** writes the
same text to a file named after the section. The file is a list of the
section's controls - the slug each is addressed by, the name it is shown under,
and what it is set to - which makes it a way to hand somebody the founding
party you are using, or to keep a set of terrain numbers beside a screenshot.

A section with nothing to save gets neither button: a roster, a graph and a
list of the dead would all write an empty file. The buttons are dim until the
head is under the pointer, so a column of sections reads as a column of names.

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
bun run check:perf  512 256               # frame time in a browser, zoom by zoom
bun run check:render 60                   # the same drawing timed headless, phase by phase
CHROMIUM_PATH=/path/to/chrome bun run check:menu   # is the menu index still the page?
```

The menu index and the meaning table are generated, and both are committed.
After changing a panel:

```sh
bun run build && bun run index:menu && bun run build   # re-read the menus
bun run index:terms                                    # only if the menus moved
bun run index:made                                     # only if the catalog moved
```

`index:menu` reads the built page, so it wants a build before it and a build
after it, the second to bake the new index in. `index:terms` and `index:made`
need the embedding model, which they download once and cache; they print
nothing the app depends on, and skipping them costs only the Meaning switches.
`index:made` builds its list out of the catalog rather than out of a file, so
it needs no harvest first - only a rerun when a building is added.

`civsmoke` founds a settlement, runs it for the given number of days and checks
the bookkeeping: no building on water or off its own footprint, no worker a
building does not agree it employs, no deed the owner does not agree they hold,
no counter its keeper does not agree they keep, no plant growing where a
building stands, no negative or over reserved stock, no boat aground, nobody
belonging to a town that does not exist, nobody walked off the map, nobody
walled out of their own town, and no person remembering more people than they
can. It reports the towns, the rivers, the fleet, the ladder of homes, the
walls, the counters, the friendships and who is currently the richest person
alive.

`GROW_SEED`, `GROW_COLS` and `GROW_ROWS` override the world it runs on. A
settlement is chaotic enough that one run says nothing about a change, so
judging one means sweeping a spread of seeds and comparing the distributions,
not reading a single number.

`uicheck.js` loads the page in headless Chromium, exercises all three modes and
every tab, paints into a sampling box, draws on a sheet on the stage and stacks
a layer on it, steps and plays the frames and sends the sheet to a person
motion, undoes and redoes both a layer and a panel field, resizes the world,
queues a building, changes a setting the world is built from and takes each of
the three ways out of leaving it unapplied, picks a person up off the map and
puts them down again,
searches the menus for a setting and follows the result to it, folds the menu
away and back, goes fullscreen and leaves it again, checks the text scale
reaches the root font size, and fails on any console error.

## Layout

| path                | purpose                                          |
| ------------------- | ------------------------------------------------ |
| `index.html`        | shell; imports the wasm module and nothing else   |
| `styles.css`        | theme and layout                                 |
| `rust/src/*.rs`     | plant simulation core (no browser) plus the shell |
| `rust/src/civ/*.rs` | settlement: terrain, towns, people, boats, draw   |
| `rust/src/ui/*.rs`  | panels, the two pixel editors, browser settings   |
| `rust/src/bin/*.rs` | headless smoke checks                             |
| `rust/tests/*.rs`   | determinism, invariants, sheets, ramps, format    |
| `tools/uicheck.js`  | headless browser pass over every panel            |
| `pkg/`              | build output: the wasm module and its loader      |
| `ARCHITECTURE.md`   | module map, data model and pipeline diagrams      |
