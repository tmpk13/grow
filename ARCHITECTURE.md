# Architecture

## Module map

```mermaid
flowchart TD
  subgraph shell [Shell]
    main["main.js<br/>tabs, frame loop, project IO"]
    state["state.js<br/>project state, save/load"]
  end

  subgraph panels [UI panels]
    matp["ui/materialsPanel.js"]
    shdp["ui/shadingPanel.js"]
    spp["ui/speciesPanel.js"]
    wldp["ui/worldPanel.js"]
    ge["ui/gridEditor.js<br/>drawable pixel grid"]
    ctl["ui/controls.js<br/>schema driven fields"]
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

  render["render.js<br/>camera, overlays, previews"]

  main --> state
  main --> sim
  main --> render
  main --> panels
  matp --> ge
  panels --> ctl
  matp --> sampler
  shdp --> shading
  spp --> species
  spp --> sim
  wldp --> world
  sim --> world
  sim --> plant
  sim --> species
  sim --> sampler
  plant --> shading
  plant --> sampler
  plant --> util
  sampler --> util
  render --> world
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

  class Shading {
    +mid, centerDark
    +topLight, bottomDark
    +edge0, edge1, gamma
  }

  class Species {
    +id, name, sizeClass
    +slots: material slot -> sampler id
    +spawn: rate, maxCount, minSpacing
    +spread: rate, radius range
    +growth: rate range, step range, maxAge
    +form: branching, leaves, wrapping
    +limits: radius, height, tips
    +shade: tones, core depths, jitter
  }

  class World {
    +int cols, rows, cellPx, soilRow
    +Int32Array[] layers
    +footprint()
    +canClaim()
    +findSupport()
  }

  class Plant {
    +int id, col, layer
    +Segment[] segments
    +Leaf[] leaves
    +Tip[] tips
    +Uint8Array mask
    +Uint32Array sprite
    +grow()
    +raster()
  }

  State *-- Materials
  State *-- Shading
  State *-- Species
  Materials *-- Sampler
  Sim *-- World
  Sim *-- Plant
  Plant --> Species
  Plant --> World : claims cells
```

## Frame pipeline

```mermaid
sequenceDiagram
  participant L as frame loop
  participant S as Sim
  participant P as Plant
  participant V as Viewport

  L->>S: step(dt) x N (speed / tickHz)
  S->>S: spawnPhase - random spawns and spread from parents
  S->>P: grow(dt)
  P->>P: advance one tip: steer, branch, leaf, climb
  P->>S: requestSpace(radius, height)
  S->>S: World.canClaim on the size class layer
  S-->>P: granted or confined
  L->>S: processRasterQueue(budget)
  S->>P: raster() - stamp mask, then shade
  L->>V: draw(sim)
  V->>S: composite() when the buffer is dirty
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

## Occupancy rules

One grid cell can hold one item per size class layer, so a ground cover and a
tree coexist while two trees cannot. A plant claims a rectangle of cells
(footprint radius by height) and asks the sim to enlarge it as it grows; a
refused request marks the plant confined and steers its tips back inward.

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
