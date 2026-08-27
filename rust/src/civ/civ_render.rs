//! Drawing the settlement.
//!
//! Nearly everything is generated: the ground is dithered out of the sampling
//! box ramps, buildings are assembled from their own dimensions and material
//! slots, and people are three pixels wide with a palette hashed from their id.
//! Nothing here is authored, so changing a cell size or repainting a material
//! box changes the whole town.
//!
//! The one exception is a settler somebody has dropped images on. A clip stands
//! in for the generated body for as long as it is there and for exactly the
//! motion it was dropped on; everything else about the frame is unchanged.
//!
//! Sprites are cached by the values they are built from, which is why the cache
//! key carries the materials version and the cell size, and why a clip carries
//! its revision.

use std::collections::HashMap;
use std::rc::Rc;

use crate::civ::boats::Boat;
use crate::civ::buildings::{Grain, Structure};
use crate::civ::people::Person;
use crate::civ::settlement::{Building, Rect, Settlement};
use crate::civ::sprites::{motion_of, Clip, Motion};
use crate::civ::terrain::{Cell, FLOW_DIRS};
use crate::sampler::{Bands, Materials};
use crate::sim::cast_shadow;
use crate::species::SizeClass;
use crate::state::State;
use crate::util::{clamp, clamp01, hash2, hex_to_packed, lerp, mix_packed, pack_rgba};
use crate::world::World;

/// How much of the drawing is worth doing.
///
/// A map big enough to be worth having is a map most of which is off screen and
/// the rest of which is too small to read. Past a point every extra pixel of
/// detail costs a frame and buys nothing, so the drawing sheds it in stages:
/// first the flourishes, then the sprites, and finally everything but the shape
/// of the town and the color of the ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Detail {
    #[default]
    Full,
    /// No smoke, no carried loads, no lit windows: the sprites, and that is all.
    Reduced,
    /// Plants become one colored blob, people become two pixels, buildings keep
    /// their silhouette but lose their openings.
    Coarse,
    /// Shapes only. A town reads as a cluster of roofs and a forest as texture.
    Blocks,
}

impl Detail {
    /// Chosen from the camera zoom against the threshold in the view config.
    /// At the default threshold a settler is still a person at 1x and a smudge
    /// at a quarter of that.
    pub fn for_zoom(zoom: f64, threshold: f64) -> Detail {
        let t = threshold.max(0.05);
        if zoom >= t {
            Detail::Full
        } else if zoom >= t * 0.55 {
            Detail::Reduced
        } else if zoom >= t * 0.28 {
            Detail::Coarse
        } else {
            Detail::Blocks
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Detail::Full => "full",
            Detail::Reduced => "reduced",
            Detail::Coarse => "coarse",
            Detail::Blocks => "blocks",
        }
    }

    pub fn sprites(self) -> bool {
        self <= Detail::Reduced
    }

    pub fn flourishes(self) -> bool {
        self == Detail::Full
    }
}

pub struct Sprite {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
    pub ox: i32,
    pub oy: i32,
}

/// What a cached sprite was built from.
///
/// Every value the drawing would vary is in here, which is what makes a hit
/// safe to reuse. It is a plain key rather than a formatted string because the
/// lookup happens once per building and once per settler per frame, and a
/// string would mean an allocation for each of them.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpriteKey {
    Building {
        /// The catalog entry, by address: the definitions are `'static`, so two
        /// buildings of a kind share one and no other kind can collide with it.
        def: usize,
        seed: u8,
        stage: u8,
        lit: bool,
        detail: Detail,
        cell_px: i32,
        depth_px: i32,
        materials: u32,
    },
    Person {
        seed: u16,
        frame: i32,
        facing: i32,
        body_w: i32,
        body_h: i32,
        adult: bool,
    },
    /// A frame of a dropped clip, scaled for the current cell size. Nothing
    /// about the settler is in the key: every person on the same motion and
    /// frame is drawn from the same pixels, so one entry serves the whole town.
    PersonClip {
        motion: u8,
        frame: i32,
        mirror: bool,
        w: i32,
        h: i32,
        lift: i32,
        rev: u32,
    },
    Boat {
        seed: u8,
        facing: i32,
        hull_w: i32,
        hull_h: i32,
        banner: u32,
    },
}

#[derive(Default)]
pub struct SpriteCache {
    map: HashMap<SpriteKey, Rc<Sprite>>,
}

impl SpriteCache {
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

fn ramp_of(materials: &Materials, sampler_id: &str) -> Rc<Bands> {
    let bands = materials.bands(sampler_id);
    if bands.is_empty() {
        Rc::new(Bands::fallback(vec![
            pack_rgba(90, 90, 90, 255),
            pack_rgba(150, 150, 150, 255),
        ]))
    } else {
        bands
    }
}

/// One pixel of a sprite: its tone, and how far down the sprite it sits, which
/// is what decides which part of the sampling box it reads from.
fn shade(bands: &Bands, t: f64, y: i32, h: i32) -> u32 {
    let v = if h > 1 { y as f64 / (h - 1) as f64 } else { 0.5 };
    bands.pick(t, v)
}

// ---- background ----------------------------------------------------------

/// Sky, ground and water for the whole map. Cached because it only changes when
/// the terrain, the palette or the map size changes; the time of day is a tint
/// drawn over the finished frame instead.
pub fn paint_terrain(sim: &Settlement, state: &State, buf: &mut [u32]) {
    let world = sim.world();
    let cfg = &state.civ;
    let materials = &state.materials;
    let soil = ramp_of(materials, &cfg.world.soil_sampler);
    let grass = ramp_of(materials, "mat-ground");
    let rock = ramp_of(materials, "mat-stone");
    let sand = ramp_of(materials, "mat-soil");
    let sky_top = hex_to_packed(&cfg.world.sky_top);
    let sky_bottom = hex_to_packed(&cfg.world.sky_bottom);
    let water_top = hex_to_packed(&cfg.view.water_top);
    let water_deep = hex_to_packed(&cfg.view.water_deep);

    for y in 0..world.sky_px {
        let t = if world.sky_px > 1 {
            y as f64 / (world.sky_px - 1) as f64
        } else {
            0.0
        };
        let c = mix_packed(sky_top, sky_bottom, t);
        let row = (y * world.px_w) as usize;
        buf[row..row + world.px_w as usize].fill(c);
    }

    let fade = cfg.world.depth_fade;
    for y in world.sky_px..world.px_h {
        let row = clamp(
            ((y - world.sky_px) / world.depth_px) as f64,
            0.0,
            (world.rows - 1) as f64,
        ) as i32;
        let far = if world.rows > 1 {
            1.0 - row as f64 / (world.rows - 1) as f64
        } else {
            0.0
        };
        for x in 0..world.px_w {
            let col = clamp((x / world.cell_px) as f64, 0.0, (world.cols - 1) as f64) as i32;
            let i = sim.terrain.idx(col, row);
            let kind = sim.terrain.kind[i];
            let noise = (hash2(x, y, 7331) - 0.5) * 0.24;
            let c = if kind == Cell::Water as u8 {
                let depth = clamp01((cfg.terrain.water_level - sim.terrain.elev[i] as f64) * 6.0);
                mix_packed(water_top, water_deep, clamp01(depth + noise * 0.5))
            } else {
                // Fertile ground reads green, bare ground reads as soil, and
                // the two are dithered into each other rather than tiled.
                let fert = sim.terrain.fert[i] as f64;
                let ramp = if kind == Cell::Rock as u8 {
                    &rock
                } else if kind == Cell::Sand as u8 {
                    &sand
                } else if fert > 0.35 {
                    &grass
                } else {
                    &soil
                };
                let t = clamp01(0.4 + far * fade * 2.0 + noise + (fert - 0.4) * 0.25);
                // The back of the ground plane is the top of it as far as the
                // box is concerned, so a soil box reads back to front.
                ramp.pick(t, 1.0 - far)
            };
            buf[(y * world.px_w + x) as usize] = c;
        }
    }

    paint_current(sim, state, buf);
    paint_deposits(sim, state, buf);
}

/// Ripples along the rivers. Drawn into the cached ground rather than animated,
/// because what they have to say is "this water goes somewhere", and that does
/// not change.
fn paint_current(sim: &Settlement, state: &State, buf: &mut [u32]) {
    if !state.civ.view.current || sim.terrain.rivers.is_empty() {
        return;
    }
    let world = sim.world();
    let pale = mix_packed(
        hex_to_packed(&state.civ.view.water_top),
        pack_rgba(226, 240, 246, 255),
        0.45,
    );
    for row in 0..world.rows {
        for col in 0..world.cols {
            let i = sim.terrain.idx(col, row);
            if sim.terrain.river_index[i] <= 0 {
                continue;
            }
            let dir = sim.terrain.flow[i];
            if dir < 0 {
                continue;
            }
            let (dx, dy) = FLOW_DIRS[dir as usize % FLOW_DIRS.len()];
            let cx = world.anchor_x(col);
            let cy = world.anchor_y(row);
            // A short dash lying along the flow, offset by the cell hash so the
            // dashes do not line up into a grid.
            let n = hash2(col, row, 4211);
            if n > 0.42 {
                continue;
            }
            let len = ((world.cell_px as f64 * 0.4).round() as i32).max(1);
            let ox = ((n - 0.2) * world.cell_px as f64 * 0.6).round() as i32;
            let oy = ((hash2(col, row, 907) - 0.5) * world.depth_px as f64).round() as i32;
            for k in 0..len {
                let px = cx + ox + dx * k;
                let py = cy + oy + dy * k * world.depth_px / world.cell_px.max(1);
                if px < 0 || px >= world.px_w || py < 0 || py >= world.px_h {
                    continue;
                }
                let j = (py * world.px_w + px) as usize;
                buf[j] = mix_packed(buf[j], pale, 0.5 - k as f64 * 0.12);
            }
        }
    }
}

fn paint_deposits(sim: &Settlement, state: &State, buf: &mut [u32]) {
    if !state.civ.view.deposits {
        return;
    }
    let world = sim.world();
    let stone = ramp_of(&state.materials, "mat-stone");
    let clay = ramp_of(&state.materials, "mat-soil");
    let ore = ramp_of(&state.materials, "mat-metal");
    for dep in &sim.terrain.deposits {
        if dep.amount <= 0.0 {
            continue;
        }
        let ramp = match dep.kind {
            crate::civ::terrain::DepositKind::Stone => &stone,
            crate::civ::terrain::DepositKind::Clay => &clay,
            crate::civ::terrain::DepositKind::Ore => &ore,
        };
        let cx = world.anchor_x(dep.col);
        let cy = world.anchor_y(dep.row);
        let left = (dep.amount / dep.max.max(1.0)).max(0.0);
        let rx = ((world.cell_px as f64 * 0.42) * (0.5 + left * 0.5)).round().max(1.0) as i32;
        let ry = ((rx as f64 * world.depth_ratio) + 1.0).round().max(1.0) as i32;
        for y in -ry..=ry {
            for x in -rx..=rx {
                let px = cx + x;
                let py = cy + y;
                if px < 0 || px >= world.px_w || py < 0 || py >= world.px_h {
                    continue;
                }
                let n = hash2(px, py, dep.seed as i32);
                let ellipse = (x * x) as f64 / (rx * rx) as f64 + (y * y) as f64 / (ry * ry) as f64;
                if ellipse > 0.6 + n * 0.5 {
                    continue;
                }
                let t = 0.35 + n * 0.5 - (y as f64 / ry as f64) * 0.15;
                let v = (y + ry) as f64 / (2 * ry).max(1) as f64;
                buf[(py * world.px_w + px) as usize] = ramp.pick(t, v);
            }
        }
    }
}

/// Cells that get walked over often wear into a path. Drawn per frame over the
/// cached ground rather than baked into it, because it keeps changing.
fn paint_paths(sim: &Settlement, state: &State, buf: &mut [u32], step: i32) {
    if !state.civ.view.paths {
        return;
    }
    let world = sim.world();
    let color = hex_to_packed(&state.civ.view.path_color);
    for row in 0..world.rows {
        for col in 0..world.cols {
            let wear = sim.traffic[(row * world.cols + col) as usize] as f64;
            if wear < 1.2 {
                continue;
            }
            if sim.terrain.kind[sim.terrain.idx(col, row)] == Cell::Water as u8 {
                continue;
            }
            let strength = clamp01((wear - 1.2) / 8.0) * 0.55;
            let x0 = col * world.cell_px;
            let y0 = world.sky_px + row * world.depth_px;
            for y in y0..y0 + world.depth_px {
                if y < 0 || y >= world.px_h || y % step != 0 {
                    continue;
                }
                for x in x0..x0 + world.cell_px {
                    let n = hash2(x, y, 913);
                    if n > strength * 1.6 {
                        continue;
                    }
                    let i = (y * world.px_w + x) as usize;
                    buf[i] = mix_packed(buf[i], color, strength);
                }
            }
        }
    }
}

// ---- buildings -----------------------------------------------------------

fn building_key(
    state: &State,
    world: &World,
    b: &Building,
    night: bool,
    detail: Detail,
) -> SpriteKey {
    let stage = if b.built {
        9
    } else {
        (((b.work_done / b.work.max(1.0)) * 8.0).floor() as i32).min(8)
    };
    SpriteKey::Building {
        def: b.def as *const _ as usize,
        seed: (b.seed & 255) as u8,
        stage: stage as u8,
        lit: b.built && night,
        detail,
        cell_px: world.cell_px,
        depth_px: world.depth_px,
        materials: state.materials.version,
    }
}

/// Everything standing on the map is generated from its own numbers and the
/// sampling boxes. Which of the three shapes it takes is the one thing the
/// catalog says outright, because a wall and a house share nothing but the
/// projection they stand in.
pub fn building_sprite(
    cache: &mut SpriteCache,
    state: &State,
    world: &World,
    b: &Building,
    night: bool,
    detail: Detail,
) -> Rc<Sprite> {
    let key = building_key(state, world, b, night, detail);
    if let Some(hit) = cache.map.get(&key) {
        return hit.clone();
    }
    let sprite = match b.def.structure {
        Structure::Wall | Structure::Gate => wall_sprite(state, world, b),
        Structure::Stall => stall_sprite(state, world, b),
        Structure::Building => house_sprite(state, world, b, night, detail),
    };
    cache.map.insert(key, sprite.clone());
    sprite
}

/// A front wall standing on the near edge of the footprint with a roof laid
/// over the depth of it, which is the same 2.5D projection the plants stand in.
fn house_sprite(
    state: &State,
    world: &World,
    b: &Building,
    night: bool,
    detail: Detail,
) -> Rc<Sprite> {
    let def = b.def;
    let eave = ((world.cell_px as f64 * 0.26).round() as i32).max(1);
    let body_w = def.w * world.cell_px;
    let depth = def.h * world.depth_px;
    let wall_h = ((def.wall_h * world.cell_px as f64).round() as i32).max(2);
    let roof_h = ((def.roof_h * world.cell_px as f64).round() as i32).max(2);
    let w = body_w + eave * 2;
    let h = depth + wall_h + roof_h;
    let mut px = vec![0u32; (w * h) as usize];

    let wall = ramp_of(&state.materials, def.palette.wall);
    let roof = ramp_of(&state.materials, def.palette.roof);
    let trim = ramp_of(&state.materials, def.palette.trim);
    let seed = b.seed as i32;
    let progress = if b.built {
        1.0
    } else {
        clamp01(b.work_done / b.work.max(1.0))
    };
    let roof_bottom = roof_h + depth;
    let wall_top = roof_bottom;

    let mut put = |px: &mut Vec<u32>, x: i32, y: i32, c: u32| {
        if x < 0 || x >= w || y < 0 || y >= h {
            return;
        }
        px[(y * w + x) as usize] = c;
    };

    if b.built {
        // Roof: a hipped plane, lighter along the ridge, drawn over the depth
        // of the footprint so a deeper building shows more roof.
        for y in 0..roof_bottom {
            let t = y as f64 / (roof_bottom - 1).max(1) as f64;
            let inset = ((1.0 - t) * body_w as f64 * 0.22).round() as i32;
            let tone = 0.78 - t * 0.42 + (hash2(0, y, seed) - 0.5) * 0.08;
            for x in inset..w - inset {
                let c = shade(&roof, tone + (hash2(x, y, seed) - 0.5) * 0.1, y, h);
                put(&mut px, x, y, c);
            }
        }
        // Ridge and eave lines give the roof an edge without an outline pass.
        let ridge = (body_w as f64 * 0.22).round() as i32;
        for x in ridge..w - ridge {
            let c = shade(&roof, 0.95, 0, h);
            put(&mut px, x, 0, c);
        }
        for x in 0..w {
            let c = shade(&trim, 0.2, roof_bottom - 1, h);
            put(&mut px, x, roof_bottom - 1, c);
        }

        for y in wall_top..h {
            let t = (y - wall_top) as f64 / (wall_h - 1).max(1) as f64;
            for x in eave..w - eave {
                let tone = 0.68 - t * 0.34 + (hash2(x, y, seed + 7) - 0.5) * 0.09;
                let c = shade(&wall, tone, y, h);
                put(&mut px, x, y, c);
            }
        }
        if detail.flourishes() {
            paint_openings(&mut px, &mut put, w, eave, body_w, wall_top, wall_h, b.seed, &trim, def, night);
        }
    } else {
        // Under construction: corner posts first, then the wall rising with the
        // work done on it.
        let raised = (wall_h as f64 * progress).round() as i32;
        for y in h - raised..h {
            let t = (y - (h - raised)) as f64 / raised.max(1) as f64;
            for x in eave..w - eave {
                let c = shade(&wall, 0.6 - t * 0.3 + (hash2(x, y, seed + 7) - 0.5) * 0.08, y, h);
                put(&mut px, x, y, c);
            }
        }
        let post_top = h - wall_h - (roof_h as f64 * 0.4 * progress).round() as i32;
        for x in [eave, w - eave - 1, eave + body_w / 2] {
            for y in post_top..h {
                let c = shade(&trim, 0.45 + (y % 3) as f64 * 0.05, y, h);
                put(&mut px, x, y, c);
            }
        }
        for x in eave..w - eave {
            let c = shade(&trim, 0.55, post_top, h);
            put(&mut px, x, post_top, c);
        }
    }

    Rc::new(Sprite { w, h, px, ox: eave, oy: h })
}

/// Door and windows, spaced along the wall rather than placed by hand, and lit
/// from inside once it is dark.
#[allow(clippy::too_many_arguments)]
fn paint_openings(
    px: &mut Vec<u32>,
    put: &mut impl FnMut(&mut Vec<u32>, i32, i32, u32),
    w: i32,
    eave: i32,
    body_w: i32,
    wall_top: i32,
    wall_h: i32,
    seed: u32,
    trim: &Bands,
    def: &crate::civ::buildings::BuildingDef,
    lit: bool,
) {
    // A doorway is a hole in the lower half of the wall, so it reads the box
    // there rather than wherever a sprite-wide default would land.
    let dark = shade(trim, 0.08, wall_top + wall_h, wall_top + wall_h);
    let glow = pack_rgba(250, 214, 130, 255);
    let door_w = ((body_w as f64 * 0.16).round() as i32).max(1);
    let door_h = ((wall_h as f64 * 0.6).round() as i32).max(2);
    let door_x = eave + (body_w as f64 * (0.3 + (seed % 3) as f64 * 0.15)).round() as i32;
    for y in wall_top + wall_h - door_h..wall_top + wall_h {
        for x in door_x..door_x + door_w {
            put(px, x, y, dark);
        }
    }
    if wall_h < 5 || (def.housing == 0 && def.slots == 0) {
        return;
    }
    let win_h = ((wall_h as f64 * 0.22).round() as i32).max(1);
    let win_w = ((body_w as f64 * 0.12).round() as i32).max(1);
    let win_y = wall_top + ((wall_h as f64 * 0.25).round() as i32).max(1);
    let step = (win_w * 2).max((body_w as f64 / 3.0).round() as i32);
    let mut x = eave + step - win_w;
    while x < w - eave - win_w {
        if !(x + win_w > door_x - 1 && x < door_x + door_w + 1) {
            for y in win_y..win_y + win_h {
                for dx in 0..win_w {
                    put(px, x + dx, y, if lit { glow } else { dark });
                }
            }
        }
        x += step;
    }
}

/// A length of wall, and the way through one.
///
/// A wall is not a building with the roof left off: it has no eave, so two
/// pieces standing side by side join into one run, and what it shows of itself
/// is the top of it - a wall is thick, and the thickness is the only thing
/// that reads at this angle.
fn wall_sprite(state: &State, world: &World, b: &Building) -> Rc<Sprite> {
    let def = b.def;
    let body_w = (b.w * world.cell_px).max(1);
    let depth = b.h * world.depth_px;
    let cap = ((def.roof_h * world.cell_px as f64).round() as i32).max(1);
    let wall_h = ((def.wall_h * world.cell_px as f64).round() as i32).max(3);
    let w = body_w;
    let h = depth + cap + wall_h;
    let mut px = vec![0u32; (w * h) as usize];

    let face = ramp_of(&state.materials, def.palette.wall);
    let coping = ramp_of(&state.materials, def.palette.roof);
    let trim = ramp_of(&state.materials, def.palette.trim);
    let seed = b.seed as i32;
    let top = depth + cap;
    // A piece of wall rises out of the ground as it is raised, so a half built
    // ring reads as a ring being built rather than as a row of stumps.
    let progress = if b.built { 1.0 } else { clamp01(b.work_done / b.work.max(1.0)) };
    let floor = h - ((h as f64 * progress).round() as i32).max(1);

    let gate = def.structure == Structure::Gate;
    let door_w = if gate { (body_w * 3 / 5).max(2) } else { 0 };
    let door_x = (w - door_w) / 2;
    let lintel = h - ((wall_h as f64 * 0.7).round() as i32).max(2);
    let dark = shade(&trim, 0.06, h, h);

    for y in floor.max(0)..h {
        for x in 0..w {
            let inside_gate = gate && x >= door_x && x < door_x + door_w && y >= lintel;
            let c = if inside_gate {
                // The opening is drawn rather than left out: an arch you can
                // see the far side of, not a hole in the sprite.
                let t = (y - lintel) as f64 / (h - lintel).max(1) as f64;
                mix_packed(dark, shade(&face, 0.2, y, h), t * 0.35)
            } else if y < top {
                // The walk along the top, foreshortened over the depth of the
                // footprint and falling away toward the near edge.
                let t = y as f64 / top.max(1) as f64;
                shade(&coping, 0.85 - t * 0.4 + (hash2(x, y, seed) - 0.5) * 0.1, y, h)
            } else {
                let t = (y - top) as f64 / (h - top).max(1) as f64;
                let tone = 0.66 - t * 0.36 + (hash2(x, y, seed + 3) - 0.5) * 0.08;
                let seam = match def.grain {
                    // Split trunks: a dark line down every paling, and a
                    // ragged head where each one was cut.
                    Grain::Upright => {
                        let post = ((world.cell_px as f64 * 0.28).round() as i32).max(2);
                        let head = (hash2(x / post, 0, seed + 11) * 2.0) as i32;
                        if y < top + head {
                            continue;
                        }
                        x % post == 0
                    }
                    // Laid courses, offset row by row so the joints break.
                    Grain::Courses => {
                        let course = ((world.cell_px as f64 * 0.3).round() as i32).max(2);
                        let row = (y - top) / course;
                        (y - top) % course == 0 || (x + row * (course / 2 + 1)) % (course * 2) == 0
                    }
                };
                if seam {
                    shade(&face, tone - 0.22, y, h)
                } else {
                    shade(&face, tone, y, h)
                }
            };
            px[(y * w + x) as usize] = c;
        }
    }

    // The lintel over a gate, and the coping line along the top of any wall.
    if gate && lintel - 1 >= floor && lintel - 1 >= 0 {
        for x in door_x - 1..door_x + door_w + 1 {
            if x < 0 || x >= w {
                continue;
            }
            for y in lintel - 2..lintel {
                if y < 0 || y < floor {
                    continue;
                }
                px[(y * w + x) as usize] = shade(&trim, 0.5, y, h);
            }
        }
    }
    if top - 1 >= floor && top - 1 >= 0 {
        for x in 0..w {
            px[((top - 1) * w + x) as usize] = shade(&trim, 0.24, top - 1, h);
        }
    }

    Rc::new(Sprite { w, h, px, ox: 0, oy: h })
}

/// A counter under an awning: two posts, a striped cloth roof and an open
/// front. Nothing behind it, because the keeper is drawn standing there.
fn stall_sprite(state: &State, world: &World, b: &Building) -> Rc<Sprite> {
    let def = b.def;
    let eave = ((world.cell_px as f64 * 0.3).round() as i32).max(1);
    let body_w = b.w * world.cell_px;
    let depth = b.h * world.depth_px;
    let post_h = ((def.wall_h * world.cell_px as f64).round() as i32).max(3);
    let roof_h = ((def.roof_h * world.cell_px as f64).round() as i32).max(2);
    let w = body_w + eave * 2;
    let h = depth + post_h + roof_h;
    let mut px = vec![0u32; (w * h) as usize];

    let cloth = ramp_of(&state.materials, def.palette.roof);
    let timber = ramp_of(&state.materials, def.palette.trim);
    let seed = b.seed as i32;
    let progress = if b.built { 1.0 } else { clamp01(b.work_done / b.work.max(1.0)) };
    let awning = roof_h + depth;
    let stripe = ((world.cell_px as f64 * 0.25).round() as i32).max(2);

    // Posts first: a half raised stall is a pair of poles in the ground.
    let post_top = h - ((post_h as f64 * progress).round() as i32).max(1);
    for x in [eave, w - eave - 1] {
        for y in post_top..h {
            px[(y * w + x) as usize] = shade(&timber, 0.4 + (y % 3) as f64 * 0.06, y, h);
        }
    }
    if progress < 1.0 {
        return Rc::new(Sprite { w, h, px, ox: eave, oy: h });
    }

    // The awning, sloping toward the viewer, striped along its width.
    for y in 0..awning {
        let t = y as f64 / (awning - 1).max(1) as f64;
        let inset = ((1.0 - t) * body_w as f64 * 0.12).round() as i32;
        for x in inset..w - inset {
            let band = ((x + (seed & 1)) / stripe) % 2 == 0;
            let tone = if band { 0.9 } else { 0.52 };
            px[(y * w + x) as usize] = shade(&cloth, tone - t * 0.3, y, h);
        }
    }
    // A scalloped fringe along the front edge, which is what says market
    // rather than shed.
    for x in 0..w {
        if (x / (stripe / 2).max(1)) % 2 == 0 {
            let y = awning;
            if y < h {
                px[(y * w + x) as usize] = shade(&cloth, 0.4, y, h);
            }
        }
    }
    // The counter: a plank across the front at waist height.
    let counter = h - ((post_h as f64 * 0.45).round() as i32).max(1);
    for y in counter..(counter + 2).min(h) {
        for x in eave..w - eave {
            px[(y * w + x) as usize] =
                shade(&timber, if y == counter { 0.72 } else { 0.34 }, y, h);
        }
    }

    Rc::new(Sprite { w, h, px, ox: eave, oy: h })
}

/// What is actually on the counter, drawn straight into the frame rather than
/// into the sprite: the wares change every time somebody buys one, and the
/// sprite cache is keyed on things that do not.
fn draw_wares(world: &World, buf: &mut [u32], b: &Building, sx: i32, sy: i32) {
    let counter = sy - ((b.def.wall_h * world.cell_px as f64) * 0.45).round() as i32 - 2;
    let mut at = 0;
    for &res in b.def.sells {
        if b.inv[res as usize] < 1.0 {
            continue;
        }
        let color = hex_to_packed(res.def().color);
        let x = sx + 1 + at * ((world.cell_px / 3).max(2));
        for dy in 0..2 {
            for dx in 0..2 {
                let (px_x, px_y) = (x + dx, counter - dy);
                if px_x < 0 || px_x >= world.px_w || px_y < 0 || px_y >= world.px_h {
                    continue;
                }
                buf[(px_y * world.px_w + px_x) as usize] = color;
            }
        }
        at += 1;
        if at >= 3 {
            break;
        }
    }
}

// ---- people --------------------------------------------------------------

/// Three pixels wide and a head: enough to read a walk cycle, a facing and
/// whether somebody is carrying something. Colors are hashed from the person id
/// so a settler looks the same for their whole life.
pub fn person_sprite(cache: &mut SpriteCache, world: &World, p: &Person, frame: i32) -> Rc<Sprite> {
    let body_h = ((world.cell_px as f64 * 0.85).round() as i32).max(4);
    let body_w = ((world.cell_px as f64 * 0.3).round() as i32).max(2);
    let adult = p.adult();
    let key = SpriteKey::Person {
        seed: (p.seed & 1023) as u16,
        frame,
        facing: p.facing,
        body_w,
        body_h,
        adult,
    };
    if let Some(hit) = cache.map.get(&key) {
        return hit.clone();
    }

    let scale = if adult { 1.0 } else { 0.7 };
    let hh = ((body_h as f64 * scale).round() as i32).max(3);
    let ww = (((body_w as f64 * scale).round() as i32) + 1).max(2);
    let w = ww + 2;
    let h = hh + 1;
    let mut px = vec![0u32; (w * h) as usize];
    let seed = p.seed as i32;
    let skin_tone = hash2(seed, 3, 11);
    let skin = pack_rgba(
        lerp(232.0, 128.0, skin_tone).round() as i32,
        lerp(190.0, 88.0, skin_tone).round() as i32,
        lerp(160.0, 62.0, skin_tone).round() as i32,
        255,
    );
    let hue = hash2(seed, 7, 23) * 360.0;
    let shirt = person_color(p);
    let legs = hsl((hue + 40.0) % 360.0, 0.22, 0.26);
    let hair = hsl((hue + 200.0) % 360.0, 0.3, 0.18);

    let head_h = ((hh as f64 * 0.32).round() as i32).max(1);
    let leg_h = ((hh as f64 * 0.3).round() as i32).max(1);
    let x0 = 1;
    for y in 0..h {
        for x in x0..x0 + ww {
            let mut c = 0;
            if y < head_h {
                c = if y == 0 { hair } else { skin };
            } else if y < hh - leg_h {
                c = shirt;
            } else if y < hh {
                // Legs split and swap with the frame, which reads as a step.
                let left = (x as f64) < x0 as f64 + ww as f64 / 2.0;
                let lift = if frame == 1 { left } else { !left };
                c = if lift && y == hh - 1 { 0 } else { legs };
            }
            if c != 0 {
                px[(y * w + x) as usize] = c;
            }
        }
    }
    let sprite = Rc::new(Sprite { w, h, px, ox: w / 2, oy: h });
    cache.map.insert(key, sprite.clone());
    sprite
}

/// One frame of a dropped clip, scaled to the map. Nearest sampling both ways,
/// so art authored smaller than the cell keeps its edges instead of blurring
/// into it, and art authored larger loses whole pixels rather than smearing.
///
/// The sprite is cached against the clip revision rather than against the
/// pixels, which is what makes a hit cheap: the whole settlement shares one
/// entry per motion, frame and facing.
pub fn person_clip_sprite(
    cache: &mut SpriteCache,
    world: &World,
    clip: &Clip,
    motion: Motion,
    frame: i32,
    mirror: bool,
    rev: u32,
) -> Rc<Sprite> {
    let fw = clip.frame_w();
    let fh = clip.h.max(1);
    let cell = world.cell_px.max(1);
    // A clip drawn taller than a few cells is somebody's mistake, not a
    // decision, and the blit is what would pay for it.
    let h = ((cell as f64 * clip.height).round() as i32).clamp(1, cell * 8);
    let w = (((fw as f64 * h as f64) / fh as f64).round() as i32).clamp(1, cell * 8);
    let lift = (cell as f64 * clip.lift).round() as i32;
    let key = SpriteKey::PersonClip {
        motion: motion.index() as u8,
        frame,
        mirror,
        w,
        h,
        lift,
        rev,
    };
    if let Some(hit) = cache.map.get(&key) {
        return hit.clone();
    }

    let mut px = vec![0u32; (w * h) as usize];
    for y in 0..h {
        let sy = ((y as i64 * fh as i64) / h as i64).min(fh as i64 - 1) as i32;
        for x in 0..w {
            let sx = ((x as i64 * fw as i64) / w as i64).min(fw as i64 - 1) as i32;
            let sx = if mirror { fw - 1 - sx } else { sx };
            px[(y * w + x) as usize] = clip.pixel(frame, sx, sy);
        }
    }
    let sprite = Rc::new(Sprite { w, h, px, ox: w / 2, oy: h + lift });
    cache.map.insert(key, sprite.clone());
    sprite
}

/// The shirt, which is the one color that stands for a settler at any zoom.
pub fn person_color(p: &Person) -> u32 {
    hsl(hash2(p.seed as i32, 7, 23) * 360.0, 0.35, 0.42)
}

fn hsl(hue: f64, sat: f64, light: f64) -> u32 {
    let h = ((hue % 360.0) + 360.0) % 360.0 / 60.0;
    let c = (1.0 - (2.0 * light - 1.0).abs()) * sat;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = light - c / 2.0;
    let (r, g, b) = if h < 1.0 {
        (c, x, 0.0)
    } else if h < 2.0 {
        (x, c, 0.0)
    } else if h < 3.0 {
        (0.0, c, x)
    } else if h < 4.0 {
        (0.0, x, c)
    } else if h < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    pack_rgba(
        ((r + m) * 255.0).round() as i32,
        ((g + m) * 255.0).round() as i32,
        ((b + m) * 255.0).round() as i32,
        255,
    )
}

// ---- boats ---------------------------------------------------------------

/// A hull, a mast and a sail, sized off the cell like everything else. The
/// colony's banner is the sail, so a boat says where it is from at a glance.
pub fn boat_sprite(cache: &mut SpriteCache, world: &World, boat: &Boat, banner: u32) -> Rc<Sprite> {
    let hull_w = ((world.cell_px as f64 * 1.5).round() as i32).max(4);
    let hull_h = ((world.cell_px as f64 * 0.4).round() as i32).max(2);
    let mast_h = ((world.cell_px as f64 * 1.1).round() as i32).max(3);
    let key = SpriteKey::Boat {
        seed: (boat.seed & 255) as u8,
        facing: boat.facing,
        hull_w,
        hull_h,
        banner,
    };
    if let Some(hit) = cache.map.get(&key) {
        return hit.clone();
    }
    let w = hull_w;
    let h = hull_h + mast_h;
    let mut px = vec![0u32; (w * h) as usize];
    let hull = pack_rgba(96, 68, 44, 255);
    let rail = pack_rgba(140, 104, 66, 255);
    let mast = pack_rgba(74, 54, 36, 255);
    let put = |px: &mut Vec<u32>, x: i32, y: i32, c: u32| {
        if x >= 0 && x < w && y >= 0 && y < h {
            px[(y * w + x) as usize] = c;
        }
    };
    // Sail: a triangle hanging off the mast, leaning the way the boat points.
    let mast_x = if boat.facing > 0 { w / 3 } else { w - 1 - w / 3 };
    for y in 0..mast_h {
        put(&mut px, mast_x, y, mast);
        let reach = ((mast_h - y) as f64 * 0.75).round() as i32;
        for k in 1..=reach {
            let x = if boat.facing > 0 { mast_x + k } else { mast_x - k };
            let tone = 0.75 + (hash2(x, y, boat.seed as i32) - 0.5) * 0.3;
            put(&mut px, x, y, mix_packed(banner, pack_rgba(255, 255, 255, 255), tone * 0.35));
        }
    }
    for y in 0..hull_h {
        let inset = if y == hull_h - 1 { 1 } else { 0 };
        for x in inset..w - inset {
            let c = if y == 0 { rail } else { hull };
            put(&mut px, x, mast_h + y, c);
        }
    }
    let sprite = Rc::new(Sprite { w, h, px, ox: w / 2, oy: h });
    cache.map.insert(key, sprite.clone());
    sprite
}

// ---- compositing ---------------------------------------------------------

/// How much of a settler is under the water, as a fraction of their height.
const WADE_DEPTH: f64 = 0.45;

/// The same as `blit`, with the bottom `sunk` of the sprite left undrawn. What
/// is under the water is not drawn at all rather than tinted: the water is
/// already painted there, and a settler half in it reads better as a shape
/// cut off at the surface than as a shape showing through it.
fn blit_above(buf: &mut [u32], world: &World, sprite: &Sprite, sx: i32, sy: i32, sunk: f64) {
    let keep = ((sprite.h as f64) * (1.0 - sunk)).round().max(1.0) as i32;
    let x0 = sx - sprite.ox;
    let y0 = sy - sprite.oy;
    for y in 0..keep.min(sprite.h) {
        let py = y0 + y;
        if py < 0 || py >= world.px_h {
            continue;
        }
        let srow = (y * sprite.w) as usize;
        let drow = (py * world.px_w) as usize;
        for x in 0..sprite.w {
            let v = sprite.px[srow + x as usize];
            if v == 0 {
                continue;
            }
            let px = x0 + x;
            if px < 0 || px >= world.px_w {
                continue;
            }
            buf[drow + px as usize] = v;
        }
    }
}

fn blit(buf: &mut [u32], world: &World, sprite: &Sprite, sx: i32, sy: i32) {
    let x0 = sx - sprite.ox;
    let y0 = sy - sprite.oy;
    for y in 0..sprite.h {
        let wy = y0 + y;
        if wy < 0 || wy >= world.px_h {
            continue;
        }
        let srow = (y * sprite.w) as usize;
        let drow = (wy * world.px_w) as usize;
        for x in 0..sprite.w {
            let v = sprite.px[srow + x as usize];
            if v == 0 {
                continue;
            }
            let wx = x0 + x;
            if wx < 0 || wx >= world.px_w {
                continue;
            }
            buf[drow + wx as usize] = v;
        }
    }
}

fn draw_pile(world: &World, buf: &mut [u32], pile: &crate::civ::settlement::Pile) {
    let color = hex_to_packed(pile.res.def().color);
    let cx = world.anchor_x(pile.col);
    let cy = world.anchor_y(pile.row);
    let size = clamp(
        (pile.n.sqrt() * 0.6).round(),
        1.0,
        (world.cell_px as f64 * 0.5).round(),
    ) as i32;
    for y in -size..=0 {
        for x in -size..=size {
            if x.abs() + y.abs() > size + 1 {
                continue;
            }
            let px = cx + x;
            let py = cy + y;
            if px < 0 || px >= world.px_w || py < 0 || py >= world.px_h {
                continue;
            }
            let n = hash2(px, py, pile.seed as i32);
            buf[(py * world.px_w + px) as usize] =
                mix_packed(color, pack_rgba(0, 0, 0, 255), 0.25 * n);
        }
    }
}

fn draw_smoke(
    world: &World,
    buf: &mut [u32],
    b: &Building,
    sprite: &Sprite,
    sx: i32,
    sy: i32,
    time: f64,
) {
    if b.def.smoke == 0 {
        return;
    }
    if time - b.active > 3.0 {
        return;
    }
    let top = sy - sprite.oy;
    let x = sx - sprite.ox + (sprite.w as f64 * 0.7).round() as i32;
    let puffs = b.def.smoke * 3;
    for i in 0..puffs {
        let phase = (time * 6.0 + (i * 3) as f64 + (b.seed % 7) as f64) % 18.0;
        let py = (top as f64 - phase).round() as i32;
        let px = x + (((phase + (b.seed % 5) as f64) * 0.5).sin() * 1.6).round() as i32;
        if px < 0 || px >= world.px_w || py < 0 || py >= world.px_h {
            continue;
        }
        let fade = 1.0 - phase / 18.0;
        let i2 = (py * world.px_w + px) as usize;
        buf[i2] = mix_packed(buf[i2], pack_rgba(210, 210, 205, 255), 0.5 * fade);
    }
}

fn draw_carry(world: &World, buf: &mut [u32], p: &Person, sprite: &Sprite, sx: i32, sy: i32) {
    let res = match p.carry.res {
        Some(res) => res,
        None => return,
    };
    let color = hex_to_packed(res.def().color);
    let size = clamp((sprite.w as f64 * 0.4).round(), 1.0, 3.0) as i32;
    let x0 = sx + if p.facing > 0 { sprite.ox } else { -sprite.ox - size + 1 };
    let y0 = sy - sprite.oy + (sprite.h as f64 * 0.35).round() as i32;
    for y in 0..size {
        for x in 0..size {
            let px = x0 + x;
            let py = y0 + y;
            if px < 0 || px >= world.px_w || py < 0 || py >= world.px_h {
                continue;
            }
            buf[(py * world.px_w + px) as usize] = if y == 0 {
                mix_packed(color, pack_rgba(255, 255, 255, 255), 0.25)
            } else {
                color
            };
        }
    }
}

/// The ground under everything: terrain, worn paths and the contact shadows of
/// whatever stands on it. Shadows are the expensive part of a frame, and they
/// only change when a plant grows or a building goes up, so they are kept here
/// and reused until something says otherwise.
fn ensure_ground(sim: &mut Settlement, state: &State) {
    let len = sim.buffer.len();
    let world_px = (sim.world().px_w, sim.world().px_h);
    let bg_key = format!(
        "{}x{}:{}:{}:{}:{}:{}",
        world_px.0,
        world_px.1,
        state.materials.version,
        state.civ.view.water_top,
        state.civ.view.water_deep,
        state.civ.view.current,
        sim.terrain_version
    );
    if sim.bg.len() != len || sim.bg_key != bg_key {
        let mut bg = vec![0u32; len];
        paint_terrain(sim, state, &mut bg);
        sim.bg = bg;
        sim.bg_key = bg_key;
        sim.ground_dirty = true;
    }
    if sim.ground.len() != len {
        sim.ground = vec![0u32; len];
        sim.ground_dirty = true;
    }
    // Only the rows the camera samples are ever read back out of the ground,
    // so only those are painted. A camera that moves closer wants the rows this
    // pass skipped, which is what makes a change of step a rebuild.
    let step = sim.px_step.max(1);
    if sim.ground_step != step {
        sim.ground_dirty = true;
    }
    // Footpaths wear in and fade slowly, so a periodic rebuild is enough to
    // keep them current without paying for them every frame.
    sim.ground_age += 1;
    if !sim.ground_dirty && sim.ground_age < 45 {
        return;
    }
    sim.ground_age = 0;
    sim.ground_dirty = false;
    sim.ground_step = step;
    let mut ground = std::mem::take(&mut sim.ground);
    if step == 1 {
        ground.copy_from_slice(&sim.bg);
    } else {
        let w = world_px.0 as usize;
        let mut y = 0;
        while y < world_px.1 {
            let a = y as usize * w;
            ground[a..a + w].copy_from_slice(&sim.bg[a..a + w]);
            y += step;
        }
    }
    paint_paths(sim, state, &mut ground, step);
    // Shadows are the single most expensive thing on the ground and the first
    // thing that stops being visible when the camera pulls back.
    if state.civ.world.shadows && sim.detail.sprites() {
        let world = sim.world();
        for plant in &sim.plant_sim.plants {
            if plant.size_class == SizeClass::Ground || plant.radius_px <= 1.0 {
                continue;
            }
            cast_shadow(
                world,
                &mut ground,
                world.anchor_x(plant.col),
                world.anchor_y(plant.row),
                plant,
            );
        }
        for b in &sim.buildings {
            building_shadow(world, &mut ground, b);
        }
    }
    sim.ground = ground;
}

fn building_shadow(world: &World, buf: &mut [u32], b: &Building) {
    let x0 = b.col * world.cell_px;
    let x1 = x0 + b.w * world.cell_px;
    let y1 = world.sky_px + (b.row + b.h) * world.depth_px;
    let drop = ((world.depth_px as f64 * 0.8).round() as i32).max(1);
    let dark = pack_rgba(6, 10, 14, 255);
    for y in y1..y1 + drop {
        if y < 0 || y >= world.px_h {
            continue;
        }
        let t = (y - y1) as f64 / drop as f64;
        for x in x0 - 1..x1 + 1 {
            if x < 0 || x >= world.px_w {
                continue;
            }
            if hash2(x, y, b.seed as i32) < t * 0.9 {
                continue;
            }
            let i = (y * world.px_w + x) as usize;
            buf[i] = mix_packed(buf[i], dark, 0.35 * (1.0 - t));
        }
    }
}

/// One thing to draw, and where in its own list it lives. Sorted by the row it
/// stands on, so the map is painted back to front.
pub(crate) enum Item {
    Plant(usize),
    Pile(usize),
    Building(usize),
    Person(usize),
    Boat(usize),
}

/// How far into the map something stands, as sixteenths of a cell, which is
/// what everything on the ground is sorted by.
///
/// Whole rows are not enough. A settler and a bush in the same row tie on the
/// row and are then separated by what kind of thing they are, which put every
/// settler in front of every plant they were standing among - so somebody
/// walking behind a bush walked over it. A plant stands in the middle of its
/// cell and a settler anywhere in theirs, and at a sixteenth of a cell that
/// difference is what decides which is in front.
pub fn depth_key(cells: f64) -> i32 {
    (cells * 16.0).round() as i32
}

/// A filled rectangle, which is what a building is at the zoom where its walls
/// are a pixel high.
fn fill_rect(buf: &mut [u32], world: &World, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
    let x0 = x0.max(0);
    let y0 = y0.max(0);
    let x1 = x1.min(world.px_w);
    let y1 = y1.min(world.px_h);
    for y in y0..y1 {
        let row = (y * world.px_w) as usize;
        for x in x0..x1 {
            buf[row + x as usize] = color;
        }
    }
}

/// A plant at a distance: one dab of its own average color, foreshortened the
/// way its shadow would be.
fn draw_plant_blob(buf: &mut [u32], world: &World, plant: &crate::plant::Plant, detail: Detail) {
    if plant.tint == 0 {
        return;
    }
    let cx = world.anchor_x(plant.col);
    let cy = world.anchor_y(plant.row);
    if detail == Detail::Blocks {
        // Two pixels rather than one: at this zoom a whole forest has to read
        // as a texture over the ground, and a single dot does not survive the
        // downscale.
        for x in cx..cx + 2 {
            if x < 0 || x >= world.px_w || cy < 0 || cy >= world.px_h {
                continue;
            }
            buf[(cy * world.px_w + x) as usize] = plant.tint;
        }
        return;
    }
    let rx = clamp((plant.radius_px * 0.7).round(), 1.0, 6.0) as i32;
    let ry = clamp((rx as f64 * world.depth_ratio).round(), 1.0, 4.0) as i32;
    let lift = (plant.height_px * 0.35).round() as i32;
    for y in -ry..=ry {
        for x in -rx..=rx {
            if x * x * ry * ry + y * y * rx * rx > rx * rx * ry * ry {
                continue;
            }
            let px = cx + x;
            let py = cy + y - lift;
            if px < 0 || px >= world.px_w || py < 0 || py >= world.px_h {
                continue;
            }
            buf[(py * world.px_w + px) as usize] = plant.tint;
        }
    }
}

/// Somebody is home: a warm smudge at the door, which is the only thing that
/// distinguishes a house with a family in it from an empty one.
fn draw_occupancy(buf: &mut [u32], world: &World, b: &Building, sx: i32, sy: i32) {
    if b.occupants <= 0 {
        return;
    }
    let glow = pack_rgba(252, 216, 138, 255);
    let w = ((b.w * world.cell_px) as f64 * 0.22).round().max(1.0) as i32;
    let x0 = sx - w / 2;
    for y in sy - 2..sy {
        for x in x0..x0 + w {
            if x < 0 || x >= world.px_w || y < 0 || y >= world.px_h {
                continue;
            }
            let i = (y * world.px_w + x) as usize;
            buf[i] = mix_packed(buf[i], glow, 0.45);
        }
    }
}

/// One frame: the cached ground, then everything standing on it in back to
/// front order. Only the part of the map the camera can see is touched, which
/// is what makes a map of a hundred thousand cells cost the same as a small
/// one to draw.
pub fn composite_settlement(sim: &mut Settlement, state: &State) {
    ensure_ground(sim, state);
    let world_rect = Rect::whole(sim.world());
    let view = if state.civ.view.cull { sim.view } else { world_rect };
    let view = Rect {
        x0: view.x0.max(0),
        y0: view.y0.max(0),
        x1: view.x1.min(world_rect.x1),
        y1: view.y1.min(world_rect.y1),
    };
    if view.is_empty() {
        sim.buffer_dirty = false;
        return;
    }
    let detail = sim.detail;
    let px_w = sim.world().px_w as usize;
    // Zoomed out the upload samples one row in `px_step`, and a row it will not
    // sample is a row nothing can read: erasing last frame's drawing there buys
    // nothing. The grid is aligned to the origin, which is where the upload
    // starts its own, so the rows that survive here are exactly the rows that
    // get read.
    let step = sim.px_step.max(1);
    let mut buf = std::mem::take(&mut sim.buffer);
    // Only the visible band is refreshed. Everything outside it is stale and
    // never uploaded.
    let first = view.y0 + (step - view.y0 % step) % step;
    let mut y = first;
    while y < view.y1 {
        let row = y as usize * px_w;
        let a = row + view.x0 as usize;
        let b = row + view.x1 as usize;
        buf[a..b].copy_from_slice(&sim.ground[a..b]);
        y += step;
    }

    // Kept between frames: on a full map this is one entry per plant, and
    // growing it from nothing every frame is the largest allocation the drawing
    // makes.
    let mut items = std::mem::take(&mut sim.items);
    items.clear();
    let world = sim.world();
    let cell = world.cell_px;
    let depth = world.depth_px;
    let sky = world.sky_px;
    for (i, plant) in sim.plant_sim.plants.iter().enumerate() {
        // A conservative box: the anchor plus the sprite's own extent.
        let ax = world.anchor_x(plant.col);
        let ay = world.anchor_y(plant.row);
        let r = plant.radius_px.max(2.0) as i32 + 2;
        let up = plant.height_px as i32 + 2;
        if !view.overlaps(ax - r, ay - up, ax + r + 1, ay + r + 1) {
            continue;
        }
        items.push((depth_key(plant.row as f64 + 0.5), 1, plant.id, Item::Plant(i)));
    }
    if detail < Detail::Coarse {
        for (i, pile) in sim.piles.iter().enumerate() {
            let ax = world.anchor_x(pile.col);
            let ay = world.anchor_y(pile.row);
            if !view.overlaps(ax - cell, ay - cell, ax + cell, ay + depth) {
                continue;
            }
            items.push((depth_key(pile.row as f64 + 0.5), 0, pile.id, Item::Pile(i)));
        }
    }
    for (i, b) in sim.buildings.iter().enumerate() {
        let x0 = b.col * cell - cell;
        let x1 = x0 + (b.w + 2) * cell;
        let y1 = sky + (b.row + b.h) * depth + depth;
        let y0 = y1 - ((b.def.wall_h + b.def.roof_h) * cell as f64) as i32 - b.h * depth - depth * 2;
        if !view.overlaps(x0, y0, x1, y1) {
            continue;
        }
        items.push((depth_key((b.row + b.h) as f64 - 0.5), 2, b.id, Item::Building(i)));
    }
    if state.civ.view.people {
        for (i, p) in sim.people.iter_indexed() {
            // Anybody indoors or at sea is not on the ground to be drawn.
            if p.indoors() || p.aboard != 0 {
                continue;
            }
            let ax = (p.x * cell as f64) as i32;
            let ay = sky + (p.y * depth as f64) as i32;
            if !view.overlaps(ax - cell, ay - cell * 2, ax + cell, ay + depth) {
                continue;
            }
            items.push((depth_key(p.y), 3, p.id as i32, Item::Person(i)));
        }
    }
    if state.civ.view.boats {
        for (i, boat) in sim.boats.iter().enumerate() {
            let ax = (boat.x * cell as f64) as i32;
            let ay = sky + (boat.y * depth as f64) as i32;
            if !view.overlaps(ax - cell * 2, ay - cell * 3, ax + cell * 2, ay + depth * 2) {
                continue;
            }
            items.push((depth_key(boat.y), 1, boat.id, Item::Boat(i)));
        }
    }
    items.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let night = sim.night_lights(state);
    let time = sim.time;
    let smoke_on = state.civ.view.smoke && detail.flourishes();
    let mut sprites = std::mem::take(&mut sim.sprites);
    for (_, _, _, item) in &items {
        let world = &sim.plant_sim.world;
        match item {
            Item::Plant(i) => {
                if detail.sprites() {
                    sim.plant_sim.blit_plant(&mut buf, *i, false);
                } else {
                    draw_plant_blob(&mut buf, world, &sim.plant_sim.plants[*i], detail);
                }
            }
            Item::Pile(i) => draw_pile(world, &mut buf, &sim.piles[*i]),
            Item::Building(i) => {
                let b = &sim.buildings[*i];
                let sx = b.col * world.cell_px;
                let sy = world.sky_px + (b.row + b.h) * world.depth_px;
                if detail == Detail::Blocks {
                    let roof = ramp_of(&state.materials, b.def.palette.roof);
                    let wall_px = ((b.def.wall_h + b.def.roof_h) * world.cell_px as f64) as i32;
                    let top = sy - wall_px.max(2);
                    fill_rect(
                        &mut buf,
                        world,
                        sx,
                        top,
                        sx + b.w * world.cell_px,
                        sy,
                        // One color for a whole building, so it reads the
                        // middle of the box rather than any part of it.
                        roof.pick(if b.built { 0.62 } else { 0.3 }, 0.5),
                    );
                    continue;
                }
                let lit = night && detail.flourishes();
                let sprite = building_sprite(&mut sprites, state, world, b, lit, detail);
                blit(&mut buf, world, &sprite, sx + sprite.ox, sy);
                if detail.flourishes() {
                    draw_occupancy(&mut buf, world, b, sx + b.w * world.cell_px / 2, sy);
                    if b.def.structure == Structure::Stall {
                        draw_wares(world, &mut buf, b, sx, sy);
                    }
                }
                if smoke_on {
                    draw_smoke(world, &mut buf, b, &sprite, sx + sprite.ox, sy, time);
                }
            }
            Item::Person(i) => {
                let p = &sim.people[*i];
                let sx = (p.x * world.cell_px as f64).round() as i32;
                let sy = (world.sky_px as f64 + p.y * world.depth_px as f64).round() as i32;
                if !detail.sprites() {
                    draw_person_dot(&mut buf, world, p, sx, sy, detail);
                    continue;
                }
                let swimming = sim.in_water(p.cell_col(), p.cell_row());
                let motion = motion_of(p, swimming);
                let sprite = match state.civ.sprites.resolve(motion) {
                    Some((slot, clip)) => {
                        let frame = clip.frame_index(p.bob, time);
                        let mirror = clip.flip && p.facing < 0;
                        person_clip_sprite(
                            &mut sprites,
                            world,
                            clip,
                            slot,
                            frame,
                            mirror,
                            state.civ.sprites.rev,
                        )
                    }
                    None => {
                        let frame = (p.bob.floor() as i64).rem_euclid(2) as i32;
                        let frame = if p.path.is_empty() { 0 } else { frame };
                        person_sprite(&mut sprites, world, p, frame)
                    }
                };
                if swimming {
                    // In the water, only what is above the waterline is drawn,
                    // so somebody crossing a river is in it rather than on it.
                    blit_above(&mut buf, world, &sprite, sx, sy, WADE_DEPTH);
                } else {
                    blit(&mut buf, world, &sprite, sx, sy);
                    if p.carrying() && detail.flourishes() {
                        draw_carry(world, &mut buf, p, &sprite, sx, sy);
                    }
                }
            }
            Item::Boat(i) => {
                let boat = &sim.boats[*i];
                let banner = sim
                    .colony_index(boat.colony)
                    .map(|ci| sim.colonies[ci].banner)
                    .unwrap_or(pack_rgba(210, 210, 214, 255));
                let (sx, sy) = crate::civ::boats::boat_anchor(sim, boat);
                if !detail.sprites() {
                    fill_rect(&mut buf, world, sx - 1, sy - 1, sx + 2, sy + 1, banner);
                    continue;
                }
                let sprite = boat_sprite(&mut sprites, world, boat, banner);
                blit(&mut buf, world, &sprite, sx, sy);
            }
        }
    }
    sim.sprites = sprites;
    sim.buffer = buf;
    sim.items = items;
    sim.buffer_dirty = false;
}

/// A settler at a distance. Two pixels of their shirt at coarse detail, one at
/// the furthest, which is enough to see a crowd move.
fn draw_person_dot(buf: &mut [u32], world: &World, p: &Person, sx: i32, sy: i32, detail: Detail) {
    let color = person_color(p);
    let h = if detail == Detail::Blocks { 1 } else { 2 };
    for y in sy - h..sy {
        if y < 0 || y >= world.px_h || sx < 0 || sx >= world.px_w {
            continue;
        }
        buf[(y * world.px_w + sx) as usize] = color;
    }
}

/// Where a building label belongs on screen, in world pixels, and what it says.
pub fn building_labels(sim: &Settlement) -> Vec<(f64, f64, String)> {
    let world = sim.world();
    sim.buildings
        .iter()
        .map(|b| {
            let x = (b.col * world.cell_px) as f64 + (b.w * world.cell_px) as f64 / 2.0;
            let y = (world.sky_px + (b.row + b.h) * world.depth_px) as f64;
            let text = if b.built {
                b.label()
            } else if b.upgrading {
                format!("{} {}%", b.def.label, ((b.work_done / b.work) * 100.0).round())
            } else {
                format!("{} {}%", b.def.label, ((b.work_done / b.work) * 100.0).round())
            };
            (x, y, text)
        })
        .collect()
}

/// One label per town, placed over its center. Drawn at every zoom, because at
/// the zoom where the buildings are blocks this is the only thing that says
/// which block is which town.
pub fn colony_labels(sim: &Settlement) -> Vec<(f64, f64, String, u32)> {
    let world = sim.world();
    sim.colonies
        .iter()
        .map(|c| {
            let x = (c.center.0 * world.cell_px) as f64 + world.cell_px as f64 / 2.0;
            let y = (world.sky_px + c.center.1 * world.depth_px) as f64;
            let pop = sim.colony_population(c.id);
            (x, y, format!("{} ({pop})", c.name), c.banner)
        })
        .collect()
}

