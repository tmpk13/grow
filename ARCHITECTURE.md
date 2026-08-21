# Architecture

## Module map

The project has two halves that share one state: the plant lab authors species
and materials, and the settlement runs a world made of them.

```mermaid
flowchart TD
  subgraph shell [Shell]
    main["main.js<br/>modes, tabs, frame loop, project IO"]
    state["state.js<br/>project state, save/load"]
  end

  subgraph labpanels [Lab panels]
    matp["ui/materialsPanel.js"]
    shdp["ui/shadingPanel.js"]
    spp["ui/speciesPanel.js"]
    wldp["ui/worldPanel.js"]
    ge["ui/gridEditor.js<br/>drawable pixel grid"]
    ctl["ui/controls.js<br/>schema driven fields"]
  end

  subgraph civpanels [Settlement panels]
    lndp["ui/landPanel.js"]
    pplp["ui/peoplePanel.js"]
    bldp["ui/buildPanel.js"]
    ecop["ui/economyPanel.js"]
    tchp["ui/techPanel.js"]
  end

  subgraph core [Simulation core, no DOM]
    sim["sim.js<br/>spawn, schedule, composite"]
    world["world.js<br/>cell grid, layer occupancy"]
    plant["plant.js<br/>growth, raster, shade"]
    species["species.js<br/>definitions, schema, limits"]
    sampler["sampler.js<br/>sampling boxes, ramps"]
    shading["shading.js<br/>tone curve"]
    rng["rng.js"]
    util["util.js<br/>color, distance transform, labels"]
  end

  subgraph civ [Settlement, no DOM]
    sett["civ/settlement.js<br/>world, buildings, books"]
    tasks["civ/tasks.js<br/>what a settler does next"]
    planner["civ/planner.js<br/>what to build and where"]
    terrain["civ/terrain.js<br/>noise, deposits, fertility"]
    people["civ/people.js<br/>needs, movement"]
    econ["civ/economy.js<br/>prices, wages, caravans"]
    tech["civ/tech.js<br/>unlocks and modifiers"]
    bdefs["civ/buildings.js<br/>catalog and recipes"]
    res["civ/resources.js"]
    cfg["civ/config.js<br/>every parameter"]
    names["civ/names.js"]
  end

  render["render.js<br/>camera, overlays, previews"]
  civrender["civ/civRender.js<br/>terrain, buildings, settlers"]

  main --> state
  main --> sim
  main --> sett
  main --> render
  main --> labpanels
  main --> civpanels
  state --> cfg
  matp --> ge
  labpanels --> ctl
  civpanels --> ctl
  matp --> sampler
  shdp --> shading
  spp --> species
  wldp --> world
  lndp --> terrain
  pplp --> people
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
  sett --> people
  sett --> econ
  sett --> tech
  sett --> bdefs
  sett --> res
  sett --> civrender
  tasks --> people
  tasks --> res
  planner --> bdefs
  civrender --> sampler
```

## Data model

```mermaid
classDiagram
  class State {
    +int seed
    +Materials materials
    +Shading shading
    +Species[] species
    +ClassLimits classLimits
    +WorldConfig world
    +SimConfig sim
    +CivConfig civ
  }

  class Materials {
    +mode: multi | single
    +Atlas atlas
    +Sampler[] samplers
    +int version
  }

  class Sampler {
    +id, name, role
    +int w, h
    +Uint32Array px
    +Region region
  }

  class Species {
    +id, name, sizeClass
    +slots: material slot -> sampler id
    +spawn, spread, growth
    +form, limits, shade
  }

  class World {
    +int cols, rows
    +int cellPx, depthPx, skyPx
    +Int32Array[] layers
    +anchorX() anchorY()
    +footprint() canClaim()
  }

  class Plant {
    +int id, col, row, layer
    +Segment[] segments
    +Uint8Array mask
    +Uint32Array sprite
    +grow() raster()
  }

  class CivConfig {
    +world, terrain, people
    +work, build, economy
    +tech, start, sim, view
  }

  class Settlement {
    +Sim plantSim
    +Terrain terrain
    +Int32Array buildGrid
    +Float32Array traffic
    +Building[] buildings
    +Person[] people
    +Pile[] piles
    +Stock stock
    +Economy econ
    +TechState tech
    +step() composite()
  }

  class Terrain {
    +Float32Array elev, moist, fert
    +Uint8Array type
    +Deposit[] deposits
  }

  class Building {
    +id, type, col, row, w, h
    +bool built
    +cost, delivered, incoming
    +inv, out, workers
  }

  class Person {
    +id, name, age
    +float x, y
    +hunger, energy, health
    +home, work, profession
    +carry, task, path
  }

  State *-- Materials
  State *-- Species
  State *-- CivConfig
  Materials *-- Sampler
  Sim *-- World
  Sim *-- Plant
  Plant --> Species
  Settlement *-- Terrain
  Settlement *-- Building
  Settlement *-- Person
  Settlement --> Sim : owns a plant world
  Person --> Building : lives and works in
```

## Frame pipeline

```mermaid
sequenceDiagram
  participant L as frame loop
  participant S as Sim
  participant P as Plant
  participant V as Viewport

  L->>S: step(dt) x N (speed / tickHz)
  S->>S: spawnPhase - random cells and spread rings around parents
  S->>P: grow(dt)
  P->>P: advance one tip: steer, branch, leaf, climb
  P->>S: requestSpace(radiusCells)
  S->>S: World.canClaim on the size class layer
  S-->>P: granted or confined
  L->>S: processRasterQueue(budget)
  S->>P: raster() - stamp mask, then shade
  L->>V: draw(sim)
  V->>S: composite() when the buffer is dirty
  S->>S: sort back to front, shadow then sprite per plant
  V->>V: blit scaled, draw grid and occupancy overlays
```

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
  nd --> curve["t = mid - centerDark*C(depth)<br/>+ topLight*C(1-vert)<br/>- bottomDark*C(vert)"]
  vert --> curve
  curve --> q["quantize to tone steps<br/>plus stable per pixel jitter"]
  q --> ramp["ramp of the sampling box<br/>bound to that material"]
  ramp --> px["output pixel"]
```

The curve `C` is a smoothstep between `edge0` and `edge1` raised to `gamma`.
Narrowing the gap between the two edges widens the flat plateau, which is what
keeps the body of an object on one tone and confines the shading to a rim.

## Projection

The grid is a ground plane seen at an angle. A cell is `cellPx` wide and
`depthPx` tall on screen, so depth is foreshortened; plants stand up out of
their cell into the sky band above the far row, and are composited back to
front.

```mermaid
flowchart LR
  cell["cell (col,row)"] --> ax["screen x = col*cellPx + cellPx/2"]
  cell --> ay["screen y = skyPx + row*depthPx + depthPx/2"]
  ax --> anchor["anchor: where the plant is rooted"]
  ay --> anchor
  anchor --> shadow["contact shadow: ellipse rx by rx*depthRatio"]
  anchor --> sprite["sprite drawn with its own origin on the anchor"]
  row["row index"] --> haze["depth shade: far rows lift toward the light<br/>end of their own ramp"]
  haze --> sprite
```

## Occupancy rules

One grid cell can hold one item per size class layer, so a ground cover and a
tree coexist while two trees cannot. A plant claims a disc of cells around its
own cell and asks the sim to enlarge it as it grows; a refused request marks
the plant confined and steers its tips back inward. Height is free, only the
ground footprint is contested.

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
  C->>P: step(dt) - the wilderness grows
  C->>C: plan() every planInterval - what to build, and where
  loop every settler
    C->>T: updatePerson(dt)
    T->>T: age, hunger, energy, health
    T->>T: eat / sleep / work, in that order
    T->>C: claim a plant, a deposit, a load or a site
  end
  C->>E: prices from stock against target
  C->>E: caravan when the interval elapses and a market stands
  C->>C: research points, unlock, apply modifiers
  C->>C: on a new day: reassign labor, births, deaths, spoilage
  L->>C: composite() - cached ground, then everything standing on it
```

## What a settler does

Every task is walk, work, carry. Nothing enters the store that a person did not
carry there.

```mermaid
stateDiagram-v2
  [*] --> Choosing
  Choosing --> Eat: hungry and food in store
  Choosing --> Sleep: after work hours
  Choosing --> Forage: food short and not a food worker
  Choosing --> Gather: has a gathering workplace
  Choosing --> Station: has a workshop, farm, school or market
  Choosing --> Haul: a site or workshop needs material
  Choosing --> Pickup: a load lies on the ground
  Choosing --> Build: a site has all its material
  Choosing --> Wander: nothing to do

  Gather --> Deliver: load full
  Pickup --> Deliver
  Station --> Deliver: output bench full
  Haul --> Choosing: delivered
  Build --> Choosing: raised
  Deliver --> Choosing: dropped off
  Eat --> Choosing
  Sleep --> Choosing: morning
  Wander --> Choosing
```

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

  plank --> builds["houses, granary, school, market"]
  brick --> builds
  stone --> builds
  tool --> builds
  cloth --> builds
  food --> people["settlers eat, and are born"]
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
  pile --> rot["rots over pileLife days"]
  carry --> full{"store has room<br/>and wants it?"}
  full -->|yes| stock["stock"]
  full -->|no| pile
```

## Settlement drawing

Nothing about a building or a settler is stored as art: both are generated from
their own numbers and the sampling boxes, and cached by the values they were
built from.

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
  psprite --> frame
  frame --> night["night tint and labels<br/>drawn on the canvas, not the buffer"]
```
