# Architecture

The whole application is one Rust crate compiled to WebAssembly. Everything
below `app` and `ui` is plain Rust with no browser dependency, so the same code
runs headless in the smoke binaries and the tests; only the two top layers touch
the DOM, and they are compiled out entirely on a native target.

```mermaid
flowchart LR
  subgraph browser [Browser]
    html["index.html<br/>imports the module, nothing else"]
    css["styles.css"]
    glue["pkg/grow.js<br/>wasm-bindgen loader"]
  end
  subgraph wasm [grow.wasm]
    app["app + ui<br/>cfg(wasm32) only"]
    core["simulation core<br/>plain Rust"]
  end
  subgraph native [Native]
    bins["bin/smoke, bin/civsmoke"]
    tests["tests/"]
  end
  html --> glue --> app --> core
  css --> html
  bins --> core
  tests --> core
```

## Module map

The project has three modes over one state: the plant lab authors species and
materials, the sprite editor draws the sheets settlers can be drawn from, and
the settlement runs a world made of all of it.

```mermaid
flowchart TD
  subgraph shell [Shell]
    main["app.rs<br/>modes, tabs, frame loop, project IO"]
    state["state.rs<br/>project state, save/load"]
  end

  subgraph labpanels [Lab panels]
    matp["ui/materials_panel.rs"]
    shdp["ui/shading_panel.rs"]
    spp["ui/species_panel.rs"]
    wldp["ui/world_panel.rs"]
    ge["ui/grid_editor.rs<br/>the sampling box, drawable"]
    pnt["ui/paint.rs<br/>tools and strokes, over any surface"]
    dec["ui/decode.rs<br/>dropped files to packed pixels"]
    wheel["ui/color_wheel.rs<br/>the brush control"]
    ctl["ui/mod.rs<br/>schema driven fields"]
  end

  subgraph artpanels [Sprite editor panels]
    artp["ui/art_panel.rs<br/>brush, layers, frames, sheets"]
  end

  subgraph civpanels [Settlement panels]
    lndp["ui/land_panel.rs"]
    pplp["ui/people_panel.rs"]
    bldp["ui/build_panel.rs"]
    ecop["ui/economy_panel.rs"]
    tchp["ui/tech_panel.rs"]
    sdrop["ui/sprite_drop.rs<br/>image drop zones, one per motion"]
  end

  subgraph core [Simulation core, no browser]
    sim["sim.rs<br/>spawn, schedule, composite"]
    world["world.rs<br/>cell grid, layer occupancy"]
    plant["plant.rs<br/>growth, raster, shade"]
    species["species.rs<br/>definitions, schema, limits"]
    sampler["sampler.rs<br/>sampling boxes, weighted tone ramps"]
    art["art.rs<br/>sprite sheets: layers, frames, cels"]
    undo["undo.rs<br/>snapshots of what an edit is about to change"]
    shading["shading.rs<br/>tone curve"]
    rng["rng.rs"]
    util["util.rs<br/>color, distance transform, labels"]
  end

  subgraph civ [Settlement, no browser]
    sett["civ/settlement.rs<br/>map, buildings, towns"]
    colony["civ/colony.rs<br/>one town's books"]
    tasks["civ/tasks.rs<br/>what a settler does next"]
    planner["civ/planner.rs<br/>what to build and where"]
    terrain["civ/terrain.rs<br/>noise, rivers, deposits"]
    people["civ/people.rs<br/>record, needs, movement"]
    pdb["civ/people_db.rs<br/>the register of settlers"]
    social["civ/social.rs<br/>who has met whom, and what they made of it"]
    csprites["civ/sprites.rs<br/>dropped settler clips, one per motion"]
    boats["civ/boats.rs<br/>hulls, cargoes, voyages"]
    path["civ/pathing.rs<br/>A* over land and water"]
    econ["civ/economy.rs<br/>prices, wages, caravans"]
    tech["civ/tech.rs<br/>unlocks and modifiers"]
    bdefs["civ/buildings.rs<br/>catalog and recipes"]
    res["civ/resources.rs"]
    cfg["civ/config.rs<br/>every parameter"]
    names["civ/names.rs"]
  end

  render["render.rs<br/>camera, overlays, previews"]
  civrender["civ/civ_render.rs<br/>terrain, buildings, settlers"]

  main --> state
  main --> sim
  main --> sett
  main --> render
  main --> labpanels
  main --> artpanels
  main --> civpanels
  main --> fbox["ui/find_box.rs<br/>the search box and where it sends you"]
  fbox --> findc["find.rs<br/>menu index, ranking"]
  findc --> midx[("assets/menu-index.json<br/>harvested from the page")]
  findc --> mterms[("assets/menu-terms.json<br/>word to setting, built offline")]
  ctl -.stamps every control.-> midx
  main --> prefs["ui/prefs.rs<br/>menu fold, text scale"]
  main --> rst["ui/reset.rs<br/>clears every browser store"]
  state --> cfg
  state --> art
  matp --> ge
  artp --> art
  artp --> pnt
  artp --> wheel
  artp --> dec
  sdrop --> dec
  ge --> pnt
  pnt --> undo
  undo --> art
  undo --> sampler
  matp --> wheel
  art --> csprites
  artp --> csprites
  labpanels --> ctl
  artpanels --> ctl
  civpanels --> ctl
  matp --> sampler
  shdp --> shading
  spp --> species
  wldp --> world
  lndp --> terrain
  pplp --> people
  pplp --> sdrop
  sdrop --> csprites
  bldp --> bdefs
  ecop --> econ
  tchp --> tech
  sim --> world
  sim --> plant
  sim --> species
  sim --> sampler
  plant --> shading
  plant --> sampler
  plant --> util
  sampler --> util
  render --> world
  sett --> sim
  sett --> terrain
  sett --> tasks
  sett --> planner
  sett --> colony
  sett --> pdb
  sett --> social
  social --> people
  csprites --> people
  civrender --> csprites
  sett --> boats
  sett --> path
  sett --> bdefs
  sett --> res
  sett --> civrender
  colony --> econ
  colony --> tech
  pdb --> people
  boats --> path
  boats --> colony
  tasks --> people
  tasks --> res
  planner --> bdefs
  civrender --> sampler
```

## One map, several towns

The world is one terrain with one wilderness growing on it. A *colony* is a set
of books over that shared map: its own store, treasury, prices and research.
Buildings and people carry the id of the town they belong to, and every question
about stock, wages or what is worth building is asked of that colony rather than
of the map.

```mermaid
flowchart LR
  map["one Terrain<br/>one plant Sim<br/>one people register"]
  map --> a["colony A<br/>stock, coin, tech"]
  map --> b["colony B<br/>stock, coin, tech"]
  map --> c["colony C<br/>stock, coin, tech"]
  a -->|expedition when crowded| b
  b -->|expedition| c
  a <-->|boats along the rivers| b
  b <-->|boats| c
  a -.->|caravans, if it has a market| out((outside world))
```

A crowded, well stocked colony sends a party of its most restless adults, and
the families that follow them, to a site far enough from every other town to be
its own place. They carry founding supplies and a quarter of the treasury out of
the parent's store, and everything the parent had learned. From then on the two
diverge: they research separately, they run out of different things, and that is
what gives the boats something to carry.

## Data model

```mermaid
classDiagram
  class State {
    +u32 seed
    +Materials materials
    +ArtLibrary art
    +Shading shading
    +Vec~Species~ species
    +ClassLimits class_limits
    +WorldConfig world
    +SimSettings sim
    +CivConfig civ
  }

  class Materials {
    +MaterialMode mode
    +Grid atlas
    +Vec~Sampler~ samplers
    +u32 version
    +RefCell~RampCache~ cache
  }

  class Sampler {
    +id, name, role
    +i32 w, h
    +Vec~u32~ px
    +Region region
  }

  class Species {
    +id, name, size_class
    +slots: material slot -> sampler id
    +spawn, spread, growth
    +form, limits, shade
  }

  class World {
    +i32 cols, rows
    +i32 cell_px, depth_px, sky_px
    +Vec~Vec~i32~~ layers
    +anchor_x() anchor_y()
    +footprint() can_claim()
  }

  class Plant {
    +i32 id, col, row
    +usize layer
    +Vec~Segment~ segments
    +Vec~u8~ mask
    +Vec~u32~ sprite
    +grow() raster()
  }

  class CivConfig {
    +world, terrain, people
    +work, build, economy
    +tech, start, sim, view
    +PeopleSprites sprites
  }

  class PeopleSprites {
    +bool enabled
    +Option~Clip~ idle, walk, carry
    +Option~Clip~ work, sleep
    +u32 rev
    +resolve() set()
  }

  class Clip {
    +i32 w, h
    +Vec~u32~ px
    +i32 frames
    +f64 fps, height, lift
    +bool stride, flip
    +frame_w() pixel() frame_index()
  }

  class Task {
    <<enum>>
    Idle Sleep Eat Harvest Mine
    Pickup Deliver Haul Build Station Shop
  }

  class Structure {
    <<enum>>
    Building Wall Gate Stall
  }

  class Settlement {
    +Sim plant_sim
    +Terrain terrain
    +Vec~i32~ build_grid
    +Vec~f32~ traffic
    +Vec~Building~ buildings
    +PeopleDb people
    +Vec~Colony~ colonies
    +Vec~Boat~ boats
    +Vec~Pile~ piles
    +PlantIndex plant_index
    +PathGrid paths, water_paths
    +Rect view
    +i32 px_step
    +Detail detail
    +step() composite()
  }

  class Colony {
    +i32 id, parent
    +String name
    +center
    +Stock stock, stock_reserved
    +Economy econ
    +TechState tech
    +Mods mods
    +population, housing, storage
    +bool abandoned
  }

  class PeopleDb {
    +Vec~Person~ all
    +HashMap~u32,usize~ by_id
    +Vec~usize~ live
    +insert() retire() prune()
    +iter() archive()
  }

  class Boat {
    +i32 id, colony
    +String name
    +home_dock, dest_dock
    +Stock cargo
    +Vec~u32~ crew
    +BoatState state
  }

  class Terrain {
    +Vec~f32~ elev, moist, fert
    +Vec~u8~ kind
    +Vec~i32~ river_index
    +Vec~i8~ flow
    +Vec~River~ rivers
    +Vec~Deposit~ deposits
  }

  class Building {
    +i32 id, colony, col, row, w, h
    +BuildingDef def
    +bool built, upgrading
    +u32 owner
    +Vec~u32~ residents, guests
    +i32 occupants
    +cost, delivered, incoming
    +inv, out, workers
  }

  class Person {
    +id, given, family, colony
    +f64 x, y, age
    +hunger, energy, health
    +coin, peak_coin
    +home, owns, work, stall, inside, aboard
    +Traits traits
    +skills per profession
    +mother, father, spouse, children
    +Vec~Bond~ bonds
    +friends, rivals, regard
    +Vec~LifeEvent~ events
    +carry, task, path
  }

  class Bond {
    +u32 who
    +f32 affinity
    +i32 met
    +u32 meetings
    +bool kin
  }

  class ArtLibrary {
    +Vec~Sheet~ sheets
  }

  class Sheet {
    +id, name
    +i32 w, h, frames
    +f64 fps
    +Vec~Layer~ layers
  }

  class Layer {
    +String name
    +bool visible
    +Vec~Cel~ cels
  }

  State *-- ArtLibrary
  ArtLibrary *-- Sheet
  Sheet *-- Layer
  State *-- Materials
  State *-- Species
  State *-- CivConfig
  CivConfig *-- PeopleSprites
  PeopleSprites *-- Clip
  Materials *-- Sampler
  Sim *-- World
  Sim *-- Plant
  Plant --> Species
  Settlement *-- Terrain
  Settlement *-- Building
  Settlement *-- Colony
  Settlement *-- PeopleDb
  Settlement *-- Boat
  PeopleDb *-- Person
  Person *-- Bond
  Person *-- Task
  Bond --> Person : somebody they met
  Building --> Structure : is a
  Settlement --> Sim : owns a plant world
  Building --> Colony : belongs to
  Person --> Colony : belongs to
  Person --> Building : owns, lives and works in
  Person --> Building : keeps a stall
  Boat --> Colony : sails for
```

## The register of settlers

Everyone who has ever lived keeps a slot. Slots are stable, so an index handed
to a task this tick still means the same person next tick, and looking somebody
up by id is a hash rather than a scan of the whole population. The dead are
marked, not removed, which is what makes parentage, marriages and obituaries
answerable questions rather than strings copied out before the record was
dropped.

```mermaid
flowchart LR
  born["a child is born"] --> slot["insert: a new slot, a fresh id"]
  slot --> live["live list<br/>iteration order = birth order"]
  died["a task decides somebody has died"] --> retire["retire: dropped from the live list,<br/>the record stays where it is"]
  retire --> arch["archive: parentage, spouse,<br/>children, trades, life events"]
  arch --> prune["prune: the oldest dead are let go<br/>once the archive outgrows its cap"]
  prune --> reindex["reindex, at the one point in the day<br/>where nothing holds a slot"]
```

`retire` returning false, rather than the `alive` flag, is what decides whether
a death is counted: a task sets that flag the moment it decides somebody has
died and the burial happens afterwards, so the flag alone would count the same
death on every tick for the rest of the run.

## Frame pipeline

```mermaid
sequenceDiagram
  participant L as frame loop
  participant S as Sim
  participant P as Plant
  participant V as Viewport

  L->>S: step(state, dt) x N (speed / tick_hz)
  S->>S: spawn_phase - random cells and spread rings around parents
  S->>P: grow(dt)
  P->>P: advance one tip: steer, branch, leaf, climb
  P->>S: request_space(radius_cells)
  S->>S: World::can_claim on the size class layer
  S-->>P: granted or confined
  L->>S: process_raster_queue(budget)
  S->>P: raster() - stamp mask, then shade
  L->>V: draw(sim)
  V->>S: composite() when the buffer is dirty
  S->>S: sort back to front, shadow then sprite per plant
  V->>V: blit scaled, draw grid and occupancy overlays
```

The settlement runs the same loop, except that it is handed the visible
rectangle and the sampling step before compositing, and composites every frame
rather than only when something is marked dirty, because people move.

## Shading pass

Each plant pixel is shaded from two measurements taken inside its own shape,
not inside the whole plant:

```mermaid
flowchart LR
  mask["material id mask"] --> group["group: wood = trunk+branch+stem,<br/>foliage = leaf+leaf edge, ground"]
  group --> dt["distance transform<br/>depth from the silhouette edge"]
  group --> cc["connected components<br/>bounding box per shape"]
  dt --> nd["depth / core depth"]
  cc --> vert["vertical position in the shape"]
  nd --> curve["t = mid - center_dark*C(depth)<br/>+ top_light*C(1-vert)<br/>- bottom_dark*C(vert)"]
  vert --> curve
  curve --> q["quantize to tone steps<br/>plus stable per pixel jitter"]
  q --> ramp["tone lookup of the sampling box<br/>bound to that material"]
  ramp --> px["output pixel"]
```

The curve `C` is a smoothstep between `edge0` and `edge1` raised to `gamma`.
Narrowing the gap between the two edges widens the flat plateau, which is what
keeps the body of an object on one tone and confines the shading to a rim.

Two things about the box reach the object drawn from it: how much of the box a
color covers, and how far up the box it was drawn.

```mermaid
flowchart LR
  boxpx["the box, pixel by pixel"] --> tally["distinct colors, sorted dark to light,<br/>each weighted by how much of the box<br/>it holds"]
  tally --> pal["palette: one entry per color"]
  tally --> whole["whole box lookup"]
  boxpx --> banded["the same, per band:<br/>rows weighted by how near they are<br/>to that height of the box"]
  banded --> bands["one tone lookup per band,<br/>top of the box first"]
  pal --> panel["the swatches"]
  bands --> strip["the panel's strip,<br/>one row per band"]
  bands --> shade["shading, terrain, buildings"]
  whole --> shade
  shade --> pick["pick: how far down the object<br/>chooses the band,<br/>tone picks within it"]
```

**Coverage.** Spreading the distinct colors evenly is what made a hand drawn box
come out looking nothing like itself: a box that is nine tenths mid green and
one pixel of highlight would hand the highlight as much of the shading range as
the green, and the object would read as evenly banded whatever had been drawn.
Weighting by coverage means the drawn proportions are the rendered ones, and a
floor of one step keeps a highlight that is only ever a pixel or two from
disappearing altogether.

**Arrangement.** Reading the box as one ramp threw away where anything was
drawn, so the panel and the object never looked like each other and the grid was
hard to form any intuition about. A box is now read at eight heights: for each,
the rows are weighted by how near they are to it, and only rows that clear a
floor count at all. A color along the top of the box is therefore in the top
bands only and never reaches the foot of the object, one drawn across the middle
reaches most of the way either way, and a box whose rows are all alike reads the
same at every height, so nothing changes for a box that ignores the idea.

The bands overlap by more than half their reach, which is what keeps the change
from one to the next a shift in the palette rather than a line across the
object.

## Projection

The grid is a ground plane seen at an angle. A cell is `cellPx` wide and
`depthPx` tall on screen, so depth is foreshortened; plants stand up out of
their cell into the sky band above the far row, and are composited back to
front.

```mermaid
flowchart LR
  cell["cell (col,row)"] --> ax["screen x = col*cellPx + cellPx/2"]
  cell --> ay["screen y = sky_px + row*depth_px + depth_px/2"]
  ax --> anchor["anchor: where the plant is rooted"]
  ay --> anchor
  anchor --> shadow["contact shadow: ellipse rx by rx*depth_ratio"]
  anchor --> sprite["sprite drawn with its own origin on the anchor"]
  row["row index"] --> haze["depth shade: far rows lift toward the light<br/>end of their own ramp"]
  haze --> sprite
```

## Rivers

Rivers are cut after the noise rather than sampled out of it, because a river
has to be a *path*: something a boat can follow from one town to another. A
spring is picked in the high ground, the course is traced downhill by steepest
descent with a remembered heading, and only then is the channel cut.

```mermaid
flowchart TD
  spring["spring: high ground, dry,<br/>away from another river's head"]
  spring --> trace["trace: steepest descent,<br/>heading remembered so it does not zig-zag,<br/>a slow sideways wobble for the meander"]
  trace --> stop{"reached water<br/>or the map edge?"}
  stop -->|yes| keep["a course that goes somewhere"]
  stop -->|no| dies["peters out in a hollow"]
  keep --> length{"long enough?"}
  dies --> length
  length -->|no| discard["thrown away, nothing is cut"]
  length -->|yes| cut["cut the channel"]
  cut --> bed["bed dropped below the water line,<br/>widening downstream"]
  cut --> flow["flow direction per cell,<br/>drawn as ripples, baked into the ground"]
  cut --> banks["banks: sand at the edge,<br/>damp fertile ground behind it"]
```

The course is kept as the polyline it was cut along, so a river has a name, a
length and an answer to whether it reaches the sea. Cells carry which river they
belong to, which is what tells a jetty site on running water from one on a pond.

## Occupancy rules

One grid cell can hold one item per size class layer, so a ground cover and a
tree coexist while two trees cannot. A plant claims a disc of cells around its
own cell and asks the sim to enlarge it as it grows; a refused request marks
the plant confined and steers its tips back inward. Height is free, only the
ground footprint is contested.

A building's footprint is claimed on a second grid, and that grid is what the
pathfinder reads. A gate is the one thing that claims a cell and still lets
people cross it, so it gets a grid of its own: the passability test runs once
per neighbor per expanded cell and cannot afford a building lookup to answer it.

```mermaid
flowchart TD
  cell["cell (col,row)"]
  cell --> l0["layer 0 ground cover"]
  cell --> l1["layer 1 herb"]
  cell --> l2["layer 2 shrub"]
  cell --> l3["layer 3 tree"]
  cell --> l4["layer 4 vine"]
  l0 --> one0["at most one instance"]
  l3 --> one3["at most one instance"]
```

## Settlement tick

The settlement owns a plant world of its own and runs it first, so the
wilderness keeps growing while people work in it.

```mermaid
sequenceDiagram
  participant L as frame loop
  participant C as Settlement
  participant P as plant Sim
  participant T as tasks
  participant E as economy

  L->>C: step(dt) x N
  C->>C: refresh_colonies - one pass for every per town tally
  C->>P: step(dt) - the wilderness grows
  C->>C: rebuild the plant index on its own timer
  C->>C: plan() and plan_walls() per town every plan_interval
  loop every living settler
    C->>T: update_person(dt)
    T->>T: age, hunger, energy, health
    T->>T: eat / sleep / work, in that order
    T->>C: claim a plant, a deposit, a load or a site
  end
  C->>C: social pass on its own timer - who is standing near whom
  loop every town
    C->>E: prices from that town's stock against its target
    C->>E: caravan when the interval elapses and a market stands
    C->>C: research points, unlock, apply modifiers
  end
  C->>C: sail the boats, feed their crews
  C->>C: on a new day, per town: reassign labor, marriages,<br/>births, home upgrades, new stalls, spoilage, expeditions
  L->>C: composite(view) - the visible band only
```

## What a settler does

Every task is walk, work, carry. Nothing enters the store that a person did not
carry there.

```mermaid
stateDiagram-v2
  [*] --> Choosing
  Choosing --> Eat: hungry and food in the town's store
  Choosing --> Sleep: after work hours
  Choosing --> Forage: food short and not a food worker
  Choosing --> Gather: has a gathering workplace
  Choosing --> Station: has a workshop, farm, school, market, inn or dock
  Choosing --> Haul: a site or workshop needs material
  Choosing --> Pickup: a load lies on the ground
  Choosing --> Build: a site has all its material
  Choosing --> Shop: hungry with coin, or coin to spare and a counter nearby
  Choosing --> Wander: nothing to do

  Gather --> Deliver: load full
  Pickup --> Deliver
  Station --> Deliver: output bench full
  Haul --> Choosing: delivered
  Build --> Choosing: raised
  Deliver --> Choosing: dropped off
  Eat --> Choosing
  Shop --> Choosing: paid the keeper
  Sleep --> Choosing: morning
  Wander --> Choosing
```

Sleeping is where a settler goes indoors, which is also the only place the map
loses sight of somebody:

```mermaid
flowchart TD
  night["work hours are over"] --> home{"has a home,<br/>and is it standing?"}
  home -->|yes| walk["walk to the door"]
  walk --> inside["step inside: not drawn,<br/>the windows light up,<br/>rests a third faster"]
  home -->|no| inn{"an inn with a free room,<br/>and the coin for it?"}
  inn -->|yes| pay["pay for the room and the supper<br/>into the town's treasury"]
  pay --> inside
  inn -->|no| rough["sleep where they stand,<br/>and be unhappy about it"]
  inside --> morning["morning: step outside"]
  rough --> morning
```

## Picking a settler up

The stage is one canvas and a press on it can mean three things. The mode
decides two of them and one switch decides the third: a press draws in the
sprite editor, and in the settlement with **Move people** on it picks up
whoever is under the pointer. Everything else drags the map, including a press
on empty ground with the switch on, so turning it on does not cost the camera
the whole stage. The middle button and a held control key drag whatever the
mode is, the same way they do while drawing.

```mermaid
flowchart TD
  press["a press on the stage"] --> mode{"what is this press?"}
  mode -->|sprite editor,<br/>plain press| brush["a stroke"]
  mode -->|settlement,<br/>Move people on| who{"anybody within<br/>reach of the point?"}
  mode -->|middle, ctrl,<br/>two fingers, anything else| pan["pan, zoom, pinch"]
  who -->|no| pan
  who -->|yes| hold["hold them:<br/>give the task up properly,<br/>step outside, stop sleeping"]
  hold --> carry["the pointer carries them;<br/>the tick leaves them out"]
  carry --> put["let go: the cell they are over,<br/>or the nearest one they can stand in"]
  put --> plan["no task, no path:<br/>they plan again from<br/>where they are standing"]
```

Where somebody can be picked up from is not where they are standing. A settler's
position is their feet and the sprite is drawn standing up out of that cell, so
`person_near` halves the distance it measures upward: the reach is an oval
leaning up the screen, which is the shape of what is being pointed at. Somebody
indoors or aboard a boat is not on the map at all and is never picked up,
whatever their recorded position says.

Three things make this safe to do to a running simulation.

* **The task is given up, not dropped.** Picking somebody up calls the same
  `abandon_task` any other change of plan does, so hauled stock goes back to the
  store, a claimed plant or pile is unclaimed, a building loses its builder and
  an inn loses its guest. Clearing `task` by hand would leave the world holding
  reservations for work nobody is coming to do.
* **The tick leaves them alone.** `Settlement::held` is the id of whoever is in
  hand, and `update_person` returns early for them - beside the early return
  that already exists for anybody at sea. They still age and still get hungry,
  since being carried about is no way out of either, but they do not walk, work
  or take anything on. It sits on the settlement rather than on the person
  because only one settler is ever in hand and because a saved `Person` should
  not carry a piece of pointer state.
* **They land somewhere they can be.** Water counts - they swim out of it - and
  anything else they cannot stand in sends them to the nearest cell they can,
  which is the same `free_spot_near` a settler with no bed walks to.

Somebody can die of old age in your hand; holding is dropped rather than
dragging a body about. The switch itself lives in `app.ui` rather than in the
project: it is how somebody is using the map right now, not something about the
map, so it is not saved and undo does not step through it.

## The ladder of homes

A settler who has saved enough has their own house pulled down and rebuilt one
rung larger. Nobody plans this and no town decides it: it is one person's coin
and one person's decision, and the tower at the end of it is the mark of a
fortune rather than of a plan.

```mermaid
flowchart LR
  hut["Hut<br/>3 beds"] -->|owner saves 90 coin| house["House<br/>5 beds"]
  house -->|260 coin, needs carpentry| manor["Manor<br/>10 beds"]
  manor -->|700 coin, needs masonry| tower["Tower<br/>8 beds, a landmark"]
  tower -.->|needs architecture| tower
```

```mermaid
sequenceDiagram
  participant O as the owner
  participant T as the treasury
  participant B as the house
  participant L as laborers
  participant I as the inn

  O->>T: pays the price of the rebuild
  Note over B: def swapped for the next rung,<br/>footprint grown into its own yard,<br/>old walls salvaged into the site
  B->>O: the household loses its beds
  O->>I: takes a room, or sleeps rough
  T->>L: wages
  L->>B: carry the brick, raise the walls
  B->>O: finished; the deed was never given up
```

Wages only move once a town has a market, so a village of huts stays a village
of huts until it has one. What a settler keeps of a wage rather than spending it
back into the town the same day is the single number that decides how fast
anybody gets rich.

Rebuilds are rationed. A house on the ground is beds out of the housing stock
and a household with nowhere to sleep, so a town raises one at a time and only
starts when there is a spare bed or a free inn room for the people it displaces.

## Households

```mermaid
flowchart TD
  marry["two unattached adults of a town,<br/>old enough, not close kin"] --> couple["married"]
  couple --> home["they share a roof"]
  home --> child["children, if the town has a spare bed<br/>and food per head in store"]
  child --> grow["adult at 12, marries from 17"]
  grow --> marry
  couple --> deed["the first adult under an unowned roof<br/>takes the deed"]
  deed --> upgrade["saves, and rebuilds it larger"]
  death["an owner dies"] --> heir["the deed passes to the oldest adult<br/>still under that roof"]
```

Two details here are load bearing rather than decorative. Births are spread over
the town's couples rather than always credited to the first, or every child in
the town shares a mother and the whole next generation are siblings who cannot
marry each other. And a couple's fertility is a property of the pair - the
younger age decides, and either partner having a roof is enough - or a household
whose house is scaffolding stops having children for the duration.

## Walls and the way through

A town that has learned to fortify rings itself. The ring is a rectangle around
everything the town has built - not around its outlying camps, and not around
the wall itself, or it would push itself one cell further out on every pass and
never close.

Gates are drawn to the busiest cells of the ring and wall to the quietest, so
the ways through end up on the roads people have already worn and the blank
stretches go over ground nobody crosses.

```mermaid
flowchart TD
  box["bounding box of the town's<br/>homes, stores, workshops and civic buildings"]
  box --> ring["+ margin, clamped to the map"]
  ring --> cells["every cell of the rectangle's edge"]
  cells --> legal{"buildable, unclaimed,<br/>and clear of any door?"}
  legal -->|no| skip["skipped: the ring has a gap there"]
  legal -->|yes| worn["sorted by how worn the cell is"]
  worn --> which{"does the ring have<br/>all its gates yet?"}
  which -->|no| gate["the most worn cell,<br/>kept apart from the other gates"]
  which -->|yes| wall["the least worn cell"]
  gate --> safe
  wall --> safe{"with this piece shut, can the ground<br/>outside it still reach the town center?"}
  safe -->|no| next["try the next candidate"]
  safe -->|yes| queue["queued as a site"]
```

That last test is the whole safety rule, and it is what makes the ring
self-limiting: a piece is only ever queued if the outside can still get in
without it. A gate is passable once it is standing, so the moment one is
finished the stretches beside it are free to be closed; with no gate at all,
the last gap simply never gets filled. Nothing has to know how many holes the
ring has.

```mermaid
flowchart LR
  claim["a piece is raised"] --> grid["build_grid: claimed"]
  claim --> blocked["blocked: no plant grows in a gateway"]
  claim --> which{"a gate?"}
  which -->|yes, and finished| open["gates grid: crossable"]
  which -->|no| shut["not crossable"]
  open --> path["pathfinder and walkable()"]
  shut --> path
```

Wall pieces have a site budget of their own, separate from the planner's, or a
town that decided to ring itself would stop building anything else until the
ring was closed. A ring is also the work of a town rather than of a village: a
settlement that walls itself too early spends everything it owns on the wall
and then starves inside it, so there is a head count below which nobody starts.

## Counters

A stall is one settler's business. Nobody plans one and nobody is assigned to
keep one: a settler with coin to spare buys the counter themselves, and the
person who paid for it is the person who stands behind it.

```mermaid
sequenceDiagram
  participant K as the keeper
  participant T as the treasury
  participant S as the town store
  participant C as the counter
  participant B as a passer-by

  K->>T: pays for the stall out of their own purse
  T->>K: laborers raise it; the deed is the keeper's
  loop while the counter is thin
    K->>S: walks over, buys what the town has spare
    K->>T: at the town's price, out of their own purse
    K->>C: carries it back and puts it out
  end
  B->>C: hungry with coin, or coin to spare
  B->>K: pays the town price plus the keeper's margin
  C->>B: a meal, or something bought for its own sake
```

This is the only thing in the settlement that moves coin from one person to
another without the treasury in the middle, and it is the only use a settler
has for coin besides a roof and a meal. The margin is what a practised trader
gets away with, so keeping a stall is a trade somebody gets better at.

Only what the town has spare is ever bought for a counter: a keeper who cleared
the granary in a famine would be selling the town its own last meal back to it.
A counter whose keeper dies stays standing with nobody behind it, which is what
lets the next settler with the coin take it on rather than pay for another one.

## What people make of each other

Everyone a settler has stood near for long enough keeps a slot in that
settler's memory: when they met, how often since, and what the two of them have
come to think of one another. Both sides get their own record, written at the
same moment.

```mermaid
flowchart TD
  pass["social pass, on its own timer"] --> buckets["everyone out on the ground,<br/>bucketed by where they stand"]
  buckets --> near["pairs within the meeting radius,<br/>capped per person per pass"]
  near --> target["what they make of each other:<br/>how alike their temperaments are,<br/>+ family, + a draw that belongs to the pair"]
  target --> move["affinity moves toward it,<br/>faster if both are outgoing"]
  move --> cross{"crossed the friendship<br/>or feud line?"}
  cross -->|yes| log["one line in each of their histories"]
  move --> fold["friends, rivals, and one number<br/>for the whole ledger"]
```

The pass is bucketed and capped rather than pairwise, so a market square with
forty people in it costs forty times a small constant instead of sixteen
hundred. It draws no random numbers at all - the per pair draw is a hash of the
two ids - so it is reproducible and does not perturb the order of every other
decision in the sim.

What the bonds are actually for:

```mermaid
flowchart LR
  bond["affinity"] --> marry["who somebody marries,<br/>among people of a like age"]
  bond --> buy["whose counter they walk to"]
  bond --> mood["contentment: friends nearby<br/>against rivals"]
  bond --> stand["standing in the town"]
  kin["family bonds"] --> keep["never forgotten to<br/>make room for a stranger"]
```

Memory is capped, and the cap is a cap on people somebody merely met: the
faintest of those is dropped to make room, and a memory that is nothing but
family has no room for a stranger at all.

One detail here is load bearing rather than decorative, and it took two
demographic collapses to find. Affinity decides *between* plausible matches,
and its say falls away as two ages diverge. Weighing it against the age gap
instead - which is the obvious way to write it - marries a strong friendship
across a generation, that couple is past bearing within a few years, and a town
that ages fast enough stops having children altogether. Nothing in the
bookkeeping notices, because nothing about it is inconsistent.

The same shape of mistake sits behind how the affinity itself is computed.
Sociability already gates whether somebody looks for a spouse at all; letting
it also raise affinity with everyone turns it into a single hidden score for
how likeable a person is, and a town whose affinities favor the outgoing
marries its sociable half to itself and leaves the rest single. Each trait is
read by one part of the sim, and this is why.

## Material flow

Raw materials come out of the ground or off a plant, are carried to a store,
and are carried out again to whatever consumes them. Every arrow is somebody
walking.

```mermaid
flowchart LR
  trees["trees and shrubs"] -->|woodcutter fells| wood[wood]
  mats["ground cover<br/>cut back, regrows"] -->|forager| food[food]
  mats --> fiber[fiber]
  fields["fertile ground"] -->|farm| food
  stoneDep["stone deposit"] -->|quarry| stone[stone]
  clayDep["clay deposit"] -->|clay pit| clay[clay]
  oreDep["ore deposit"] -->|mine| ore[ore]

  wood -->|sawpit| plank[plank]
  wood -->|charcoal hearth| charcoal[charcoal]
  clay --> kiln[[kiln]]
  charcoal --> kiln
  kiln --> brick[brick]
  ore --> smelter[[smelter]]
  charcoal --> smelter
  smelter --> metal[metal]
  metal --> smithy[[smithy]]
  charcoal --> smithy
  smithy --> tool[tool]
  fiber -->|weaver| cloth[cloth]

  plank --> builds["houses, granary, school, market,<br/>inn, dock"]
  brick --> builds
  stone --> builds
  tool --> builds
  cloth --> builds
  food --> people["settlers eat, and are born"]
  food --> inn["inn: a supper with the room"]
  plank --> hull["dock: hulls"]
  wood --> hull
  hull --> boats["boats, and the trade between towns"]
```

## Boats and the water between towns

A river is only a feature until something uses it. A colony with a dock lays
down hulls there, crews them from the dock workers, and sends them to the towns
that want what it has too much of. Boats path over water cells only, so a town
on a lake trades with nobody and a town on a river that reaches the sea trades
with everyone.

```mermaid
stateDiagram-v2
  [*] --> Moored
  Moored --> Moored: nothing worth casting off for
  Moored --> Outbound: loaded with the surplus,<br/>crew aboard, a port chosen
  Outbound --> Trading: tied up at the far jetty
  Trading --> Inbound: sold the cargo at their prices,<br/>bought what home is short of
  Inbound --> Moored: landed, coin into the treasury,<br/>crew ashore
  Trading --> Moored: the way home has silted up
```

The far town's prices are the far town's, not ours, which is the whole point:
the boat sells into a shortage and buys out of a glut, and doing that repeatedly
levels two towns without either of them deciding to. Crews are not simulated
while aboard; the boat carries food for them and feeds them at sea.

```mermaid
flowchart LR
  a["colony A<br/>too much timber"] -->|load| boat(("boat"))
  boat -->|water path only| b["colony B<br/>short of timber, has ore"]
  b -->|sells at B's price| coin["coin in the hold"]
  coin -->|buys B's surplus| boat2(("boat"))
  boat2 -->|home| a
```

## Loads on the ground

A person carries one load. Anything a felled tree yields beyond that stays
where it fell until somebody comes back for it, and rots if nobody does. The
same happens to a delivery the store has no room for, which is what makes the
next storehouse worth building.

```mermaid
flowchart TD
  cut["plant felled or dug"] --> split{"fits in one load?"}
  split -->|yes| carry["carried to the store"]
  split -->|no| pile["pile left on the ground"]
  pile --> claim["a free pair of hands claims it"]
  claim --> carry
  pile --> rot["rots over pile_life days"]
  carry --> full{"store has room<br/>and wants it?"}
  full -->|yes| stock["stock"]
  full -->|no| pile
```

## Settlement drawing

Nothing about a building is stored as art: it is generated from its own numbers
and the sampling boxes, and cached by the values it was built from. A settler is
the same until somebody drops images on the people panel, at which point the
generator steps aside for the art.

```mermaid
flowchart TD
  terrain["terrain type per cell"] --> ground["ground layer<br/>sky, water, soil, grass, rock"]
  traffic["traffic per cell"] --> ground
  shadows["contact shadows of<br/>plants and buildings"] --> ground
  ground --> cache["cached until a plant grows,<br/>a building goes up or the palette changes"]
  cache --> frame["frame buffer"]
  order["sort by row, then flat before standing,<br/>then piles, buildings, people"] --> frame
  bdef["building w, h, wall and roof height<br/>in cell widths"] --> bsprite["roof plane over the footprint depth,<br/>front wall on the near edge,<br/>door and windows spaced along it"]
  ramps["sampling box ramps<br/>timber, thatch, stone, brick, cloth"] --> bsprite
  bsprite --> frame
  pseed["person id hash"] --> psprite["skin, shirt, hair<br/>two frame walk cycle"]
  psprite --> pick{"a clip dropped<br/>for this motion?"}
  clip["civ/sprites.rs<br/>sheet, frame count, rate, height"] --> pick
  pick -->|no| frame
  pick -->|yes| frame
  frame --> night["night tint and labels<br/>drawn on the canvas, not the buffer"]
```

## Finding a setting

There are eleven panels between the three modes and a few hundred controls
across them. The search box in the top bar is the way to one without knowing
which panel it is in.

Nothing about the index is written by hand. Every labeled control passes
through `ui::row`, and every button through `ui::button`, so stamping there
means the page carries a `data-find` for each of them and can be read back:

```mermaid
flowchart LR
  panels["panels build rows"] --> stamp["ui::row stamps data-find"]
  stamp --> page["the running page"]
  page --> harvest["tools/menuindex.js<br/>visits every mode and tab"]
  harvest --> midx[("assets/menu-index.json")]
  midx --> baked["include_str! into the wasm"]
  midx --> terms["tools/menu-terms<br/>embedding model, offline"]
  terms --> mterms[("assets/menu-terms.json")]
  mterms --> baked
  baked --> rank["find.rs ranks a query"]
  rank --> jump["find_box.rs: mode, tab, scroll, flash, focus"]
  jump --> page
```

The index is baked into the binary rather than fetched, so it can never be a
version behind the build it describes, and `bun run check:menu` fails if the
committed file has drifted from what the page draws.

Ranking is fuzzy by default and lives entirely in `find.rs`: a query is split
into words, each word is scored against the label, the section, the tab and the
hint at falling weights, and **every** word has to land somewhere or the entry
is dropped, so a second word narrows the list. A label the query is a prefix of
is pushed to the top whatever else happens to contain the same letters.

The **Meaning** switch adds a second score from a table of word to entry built
ahead of time. It is deliberately worth a shade less than the letters, so it can
only ever add rows, never reorder an exact match down. The table points at
entries by position, so it carries a stamp over the index it was built for and
is dropped whole if the menus have moved since; with no table the switch is not
offered at all.

Why a table and not a model: the matching is done by a static embedding model
(`potion-base-8M`, about thirty megabytes) whose crates want threads, native
TLS and a filesystem. A page compiled to WebAssembly has none of those. So
`tools/menu-terms` scores every word of the model's vocabulary against every
entry once, keeps the three or four entries each word is closest to, throws
away every word that is close to nothing (which is most of them), and ships
about eighty kilobytes of answers instead.

## The sprite editor

The sprite editor is a mode of its own, beside the plant lab and the settlement,
and a settler motion can be pointed at a sheet drawn in it instead of at a
dropped image. A sheet is a frame size, a stack of layers, and one cel per layer
per frame; drawing lands on a single cel, and what anything else reads is the
flattened frame.

Being a mode rather than a tab is what lets the drawing surface be the stage.
The sheet is composited into a flat buffer - checker, onion skin, art - and
pushed through the same camera every other mode draws through, so zoom, pan and
pinch behave the way they do on a map and the art is drawn at a whole number of
screen pixels per art pixel. The panel keeps what is around a stroke: the brush,
the layer stack, the frame strip.

```mermaid
flowchart LR
  sel["the selected sheet,<br/>layer and frame"] --> comp["one flat buffer:<br/>checker, the frame before this<br/>one faint, the frame itself"]
  comp --> cam["the camera<br/>present_flat"]
  cam --> stage["the stage canvas"]
  ptr["a pointer on the stage"] --> which{"sprite editor,<br/>plain press?"}
  which -->|yes| brush["a stroke, located through<br/>the camera rather than<br/>against the element"]
  which -->|no: middle, ctrl,<br/>or another mode| drag["pan and zoom"]
  brush --> sel
```

Sharing the stage with the camera is why the editor drives its own stroke rather
than letting `ui/paint`'s `attach` own the pointer: the same canvas has to drag
as well as draw. The middle button and a held control key are what drag there,
since the left one is busy.

```mermaid
flowchart TD
  sheet["a sheet<br/>frame size, rate"] --> layers["layers, bottom of the pile first"]
  layers --> cels["one cel per frame, each w by h"]
  cels --> flat["flatten a frame:<br/>every visible layer over the one below,<br/>topmost pixel wins"]
  flat --> ed["the editor canvas<br/>plus the frame before it, faint"]
  flat --> prev["the preview, playing at the sheet's rate"]
  flat --> strip["every frame side by side"]
  strip --> clip["a settler clip, copied<br/>rather than followed"]
  cels --> rle["saved run length encoded:<br/>count and color per run,<br/>because sprite art is mostly empty"]
```

Both pixel editors in the tool share their tools and strokes. `ui/paint.rs`
holds the pencil, eraser, fill and pick and the pointer handling; what differs
between the sampling grid and a sheet is only which buffer they land in, which
each supplies as a surface. A surface also says what the pick tool reads, which
for a stack of layers is what is on show rather than the layer being drawn on,
and how to record what an edit is about to change.

Images reach a sheet the same way they reach a settler motion, through
`ui/decode.rs`: a file becomes an object URL, an image element, a canvas and
packed pixels. A drop on the editor lands on the selected layer, scaled down to
fit the frame and centered, one image per frame from the frame being drawn.

### Stepping back

Undo is snapshots, not inverse operations, and the snapshot is the whole
project. A flood fill, a resize that crops, a merge that folds two layers
together and a world parameter that restarts a simulation are none of them
invertible on their own, and typing every settable thing as its own kind of
target would be a taxonomy nobody could keep complete.

```mermaid
flowchart TD
  ctl["a control: a field, a button,<br/>a stroke, a drop"] --> rec{"same control,<br/>within a moment?"}
  rec -->|yes, and it is one<br/>somebody holds| extend["extend the step already there,<br/>so a slider drag is one step"]
  rec -->|no| snap["copy the project as it stands"]
  snap --> done["the done stack"]
  snap --> drop["drop the undone stack:<br/>a redo would put back<br/>something this branch never had"]
  done -->|undo| swap["copy the current project<br/>into the other stack"]
  undone["the undone stack"] -->|redo| swap
  swap --> put["put the snapshot back"]
  put --> inval["drop everything cached from a project:<br/>ramps, environments, sprites, dirty flags"]
  inval --> marks{"did the world configs<br/>or the seeds move?"}
  marks -->|no| keep["the simulations carry on"]
  marks -->|yes| restart["start them again,<br/>as the forward edit would have"]
  keep --> clamp["clamp every selection:<br/>a step back can take away<br/>the sheet, layer or species<br/>being edited"]
  restart --> clamp
```

Recording happens in the eight panel helpers in `ui/mod.rs`, which every panel
builds its fields from, so every parameter in the tool is covered at once.
The controls that bypass them - a stroke, an image drop, a nudge, a layer's name
and visibility, the clip fields on a settler motion - record for themselves. The
three preview knobs in the shading panel deliberately do not: they change the
panel rather than the project, and a step that restores nothing is worse than no
step at all.

The stacks are bounded by depth and by the pixels they hold, because a project
carrying sheets near their caps is megabytes on its own.

## Settler animations

A settler can be drawn from images instead of from the generator. Every motion
the simulation can put somebody in has its own slot, its own frame count and its
own playback, so a walk and a sleep are not forced to share a cadence.

```mermaid
flowchart TD
  drop["images dropped on a motion"] --> how{"how many files?"}
  how -->|one| strip["read as a strip:<br/>frames guessed from the shape,<br/>square frames unless told otherwise"]
  how -->|several| each["one frame each, in the order<br/>their names sort, so walk2<br/>lands before walk10"]
  ed["a sheet from the sprite editor"] --> flat["flattened frame by frame"]
  each --> box["a common box, widest and tallest<br/>of them; every frame centered<br/>across it and stood on its floor"]
  strip --> trim
  box --> trim
  flat --> trim["cropped to the box the art fills<br/>in every frame, so the drawn height<br/>means the art and not the padding"]
  trim --> cap["scaled down only if a frame<br/>is over the size cap,<br/>frame by frame so the width<br/>stays a whole number of frames"]
  cap --> sheet["one sheet, kept whole"]
  sheet --> cut["frames are cut from the full width<br/>rather than stepped by the floored one,<br/>so an uneven sheet does not drift"]
  cut --> saved["saved with the project<br/>as RGBA hex, like every other<br/>pixel buffer"]
```

Padding is the thing that used to make two sheets asked for the same height come
out different sizes, and cropping to the art is what fixes it: a figure drawn a
third of the way up a large canvas is the same figure as one drawn tight, and
the clip is now sized by the figure either way. A clip can also be mirrored on
the way out, for art drawn facing the other way than the settler walks; that is
separate from mirroring when facing left, which is about the direction of
travel rather than about the sheet.

What a settler is doing folds down to one motion, and a motion with an empty
slot borrows from a related one, so a single walk sheet stands in everywhere.

```mermaid
flowchart TD
  p["a settler"] --> m{"what are they doing?"}
  m -->|sleeping| sl["sleep"]
  m -->|in the water| sw["swim"]
  m -->|on a path, loaded| ca["carry"]
  m -->|on a path| wa["walk"]
  m -->|stood at a task mid-work| wo["work"]
  m -->|otherwise| id["idle"]
  sw -.->|nothing dropped| wa
  ca -.->|nothing dropped| wa
  wa -.->|nothing dropped| id
  wo -.->|nothing dropped| id
  sl -.->|nothing dropped| id
  id -.->|nothing dropped| gen["the generated settler"]
```

A swimmer is blitted with the bottom of the sprite left undrawn, so they are in
the water rather than on it, whatever art the motion resolves to.

Which frame shows is either the clock or the ground, per clip. A walk tied to
the clock slides or runs on the spot; a walk tied to the ground covered never
does, because the same counter that made the generated settler take a step is
what advances it. Sprites are cached per motion, frame, facing and cell size
rather than per person: the whole town shares one entry, and a change to any
clip drops the cache.

## Drawing a map too big to draw

A map worth having is a map most of which is off screen and the rest of which is
too small to read. Three things keep the cost of a frame tied to the size of the
window rather than the size of the map.

**Only the visible band is touched.** The camera hands the settlement the
rectangle of world pixels it can show. That band, and only that band, is copied
out of the cached ground, composited into, and uploaded to the canvas. The rest
of the buffer stays stale, because nothing is going to look at it.

**Below one to one, only the pixels the screen will show are made.** The band
answers where; this answers how densely inside it. At a zoom of a quarter the
canvas draws one screen pixel per four by four block of world pixels and
discards the rest, so the frame stops producing them.

```mermaid
flowchart TD
  cam["camera zoom times device pixel ratio"] --> step["sample step:<br/>one below that, floor of its reciprocal"]
  step --> grid["one grid, aligned to the world origin<br/>rather than to the view, so panning slides<br/>the image instead of changing<br/>which pixels survive"]
  grid --> g["ensure_ground paints<br/>those rows only"]
  g --> c["composite restores<br/>those rows only, and draws over them"]
  c --> p["present_region gathers that pixel<br/>of each block and draws it<br/>back out at step times zoom"]
  step --> inval["a change of step rebuilds the ground:<br/>zooming in wants the rows<br/>the last pass skipped"]
```

**Detail is shed in stages as the camera pulls back.**

```mermaid
flowchart TD
  zoom["camera zoom"] --> lvl{"against the detail threshold"}
  lvl -->|at or above| full["full: everything"]
  lvl -->|above half| red["reduced: no smoke, no carried loads,<br/>no lit windows, no door openings,<br/>no contact shadows"]
  lvl -->|above a quarter| coarse["coarse: plants become one dab of their<br/>own average color, people become<br/>two pixels, piles are dropped"]
  lvl -->|below| blocks["blocks: buildings are filled roof<br/>rectangles, people one pixel,<br/>plants a two pixel dab"]
```

A plant's average color is measured once, at raster time, over a stride of its
finished sprite; drawing a forest of thousands at the furthest zoom is then one
write per plant. Town names are drawn over their centers at every zoom, because
at the zoom where several towns fit on screen they are the only thing telling
them apart.

## Finding a way

Pathing is A* over the same eight neighbors, with three things that make it
survive a hundred thousand cells: visited marks are generation stamped rather
than cleared, the frontier is a heap ordered by an octile heuristic so a search
reaches the goal instead of fanning out over the map, and every search is capped
so an unreachable target costs a bounded amount. Cells that have been walked
over are cheaper to cross, which is why traffic wears into roads that people
then prefer.

The same grid, with water as the passable set instead of land, is what the boats
use.

## Asking what is growing nearby

Every gathering decision used to walk the whole plant list. Instead a coarse
bucket grid holds a flat copy of the one thing the settlement asks about -- how
much of what, and where -- rebuilt on a timer rather than kept exact, because a
camp choosing a tree that grew a little since the last sweep is invisible.

```mermaid
flowchart LR
  plants["every plant"] -->|on a timer| marks["PlantMark: id, cell,<br/>mass, size class, who claimed it"]
  marks --> buckets["buckets of 8x8 cells"]
  buckets --> pick["pick a tree: read the buckets<br/>within reach, not the map"]
  buckets --> camp["site a camp: standing biomass<br/>near a candidate cell"]
  buckets --> staff["staff a camp: is there<br/>anything left to cut?"]
```

A task that comes back to the same plant every tick carries the slot it last
found it in, so the lookup is a bounds check rather than a scan, and the scan is
only paid for on the ticks where something was actually removed.

## Counting things once

The same shape of mistake keeps appearing: a question that reads like a field
access but is a fold over the whole world, asked inside a loop.

```mermaid
flowchart LR
  q["how many of these do we have?<br/>is there a market?<br/>how many beds?"]
  q --> bad["once per caller<br/>= a walk of every building"]
  q --> good["once per pass<br/>= a lookup"]
  good --> a["refresh_colonies:<br/>population, adults, roofless,<br/>housing, storage, market, stores"]
  good --> b["tally_types:<br/>per building type, how many stand<br/>and how many benches are empty"]
```

`has_market`, asked per working settler per tick, was a walk of every building
in the world. `plan_next` asked three such questions for each of twenty-five
building types, twice a simulated second per colony. Both are now one pass whose
result everything else reads.
