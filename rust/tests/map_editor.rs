//! The map editor: reading a set of layers in as a map, what a layer's file
//! name says it is, and where a layer says its thing is.

use grow::civ::map_brush::{lay_cells, layer_mask, read_layers, Brush, LayerMask};
use grow::civ::settlement::Settlement;
use grow::civ::sprites::{pixel_size, shrink};
use grow::civ::terrain::Cell;
use grow::state::State;
use grow::util::pack_rgba;
use grow::world::Zone;

/// A picture of `w` by `h` blocks, each `n` by `n` pixels, from a function
/// that says what color each block is.
fn blocks(w: i32, h: i32, n: i32, color: impl Fn(i32, i32) -> u32) -> (i32, i32, Vec<u32>) {
    let (pw, ph) = (w * n, h * n);
    let mut px = vec![0u32; (pw * ph) as usize];
    for y in 0..ph {
        for x in 0..pw {
            px[(y * pw + x) as usize] = color(x / n, y / n);
        }
    }
    (pw, ph, px)
}

/// A layer as a drawing program exports one: something drawn where `on` says
/// so and nothing at all elsewhere.
fn drawn(w: i32, h: i32, on: impl Fn(i32, i32) -> bool) -> Vec<bool> {
    let px: Vec<u32> = (0..w * h)
        .map(|i| if on(i % w, i / w) { pack_rgba(40, 90, 200, 255) } else { 0 })
        .collect();
    let (mask, by_light) = layer_mask(w, h, &px);
    assert!(!by_light, "a layer with clear pixels in it was read as a mask");
    mask
}

// ---- what scale a picture was drawn at -----------------------------------

#[test]
fn art_drawn_eight_pixels_to_a_pixel_reads_as_eight() {
    let (w, h, px) = blocks(16, 8, 8, |x, y| pack_rgba(x * 15, y * 30, 40, 255));
    assert_eq!(pixel_size(w, h, &px), 8);
}

#[test]
fn art_drawn_one_to_one_reads_as_one() {
    // Every pixel its own color: there are no blocks to find.
    let (w, h, px) = blocks(64, 32, 1, |x, y| pack_rgba(x * 3, y * 7, x + y, 255));
    assert_eq!(pixel_size(w, h, &px), 1);
}

#[test]
fn a_scale_that_does_not_divide_the_picture_is_not_offered() {
    // Blocks of three across a picture eleven wide: the runs share a divisor
    // that cannot tile the picture, so the honest answer is one.
    let (w, h, px) = blocks(11, 4, 3, |x, y| pack_rgba(x * 20, y * 50, 0, 255));
    let n = pixel_size(w, h, &px);
    assert!(w % n == 0 && h % n == 0, "{n} does not divide {w} by {h}");
}

#[test]
fn one_flat_color_says_nothing_about_its_scale() {
    let (w, h, px) = blocks(8, 8, 4, |_, _| pack_rgba(10, 20, 30, 255));
    assert_eq!(pixel_size(w, h, &px), 1);
}

#[test]
fn shrinking_takes_one_pixel_per_block() {
    let (w, h, px) = blocks(4, 2, 8, |x, y| pack_rgba(x * 10, y * 10, 0, 255));
    let (ow, oh, out) = shrink((w, h, px), 8);
    assert_eq!((ow, oh), (4, 2));
    assert_eq!(out[0], pack_rgba(0, 0, 0, 255));
    assert_eq!(out[3], pack_rgba(30, 0, 0, 255));
    assert_eq!(out[4], pack_rgba(0, 10, 0, 255));
}

#[test]
fn a_layer_drawn_in_blocks_says_its_scale() {
    // A blob drawn at eight pixels to a pixel on a clear layer: the runs
    // inside it are multiples of eight, and the clear runs touch the edges
    // and are left out.
    let (w, h, px) = blocks(16, 8, 8, |x, y| {
        if (4..12).contains(&x) && (2..6).contains(&y) {
            pack_rgba(x * 10, y * 10, 0, 255)
        } else {
            0
        }
    });
    assert_eq!(pixel_size(w, h, &px), 8);
}

// ---- where a layer says its thing is -------------------------------------

#[test]
fn a_layer_is_where_something_was_drawn_on_it() {
    let on = drawn(6, 4, |x, y| x == y);
    assert_eq!(on.iter().filter(|&&v| v).count(), 4);
    assert!(on[0] && on[7] && !on[1] && !on[6]);
}

#[test]
fn a_layer_with_nothing_clear_in_it_is_read_light_against_dark() {
    // Black and white, every pixel opaque: a mask rather than a drawing, and
    // the light part is the thing.
    let px: Vec<u32> = (0..12)
        .map(|i| if i < 6 { pack_rgba(250, 250, 250, 255) } else { pack_rgba(10, 10, 10, 255) })
        .collect();
    let (on, by_light) = layer_mask(6, 2, &px);
    assert!(by_light);
    assert_eq!(on[..6], [true; 6]);
    assert_eq!(on[6..], [false; 6]);
    // A faint gray is the thing too: the line is the middle of the range.
    let (on, _) = layer_mask(1, 1, &[pack_rgba(140, 140, 140, 255)]);
    assert!(on[0]);
}

#[test]
fn a_layer_short_of_pixels_is_clear_where_it_ends() {
    let (on, _) = layer_mask(4, 2, &[pack_rgba(1, 1, 1, 255), 0]);
    assert_eq!(on.len(), 8);
    assert!(on[0] && !on[1] && !on[7]);
}

// ---- what a layer is -------------------------------------------------------

#[test]
fn what_a_layer_is_comes_from_its_name() {
    assert_eq!(Brush::guess("water.png"), Brush::Water);
    assert_eq!(Brush::guess("02 Rock face.png"), Brush::Cliff);
    assert_eq!(Brush::guess("rocks.png"), Brush::Rock);
    assert_eq!(Brush::guess("Trees-2.PNG"), Brush::Wood);
    assert_eq!(Brush::guess("the_forest.webp"), Brush::Wood);
    assert_eq!(Brush::guess("sandy shore.png"), Brush::Sand);
    assert_eq!(Brush::guess("meadow.png"), Brush::Low);
    assert_eq!(Brush::guess("road.png"), Brush::Bare);
    assert_eq!(Brush::guess("grass.png"), Brush::Grass);
    assert_eq!(Brush::guess("sky.png"), Brush::Sky);
    // Whole words: nothing here is low, or a sea.
    assert_eq!(Brush::guess("yellow.png"), Brush::Clear);
    assert_eq!(Brush::guess("Layer 7.png"), Brush::Clear);
}

#[test]
fn a_choice_on_the_page_reads_back_as_the_brush_it_named() {
    for brush in grow::civ::map_brush::BRUSHES {
        assert_eq!(Brush::from_key(&brush.key()), brush, "{brush:?} did not come back");
    }
    assert_eq!(Brush::from_key("nothing-of-the-kind"), Brush::Clear);
}

// ---- layers as a map -------------------------------------------------------

#[test]
fn layers_laid_over_a_map_become_that_map() {
    let mut state = State::new();
    state.civ.world.cols = 24;
    state.civ.world.rows = 12;
    // Two layers drawn four pixels to a cell: water over the left half, a
    // wood over the right, and grass under both by default.
    let water = drawn(96, 48, |x, _| x < 48);
    let wood = drawn(96, 48, |x, _| x >= 48);
    let layers = [
        LayerMask { brush: Brush::Water, w: 96, h: 48, on: &water },
        LayerMask { brush: Brush::Wood, w: 96, h: 48, on: &wood },
    ];
    let cells = read_layers(&layers, 24, 12, Cell::Grass);
    let mut sim = Settlement::new(&state);
    lay_cells(&mut sim, &cells);

    assert_eq!(sim.terrain.type_at(2, 6), Cell::Water, "the left half is not water");
    assert_eq!(sim.terrain.zone_at(20, 6), Zone::Wood, "the right half was not zoned");
    // A zone says what may take root; it does not turn the ground into
    // anything, so the wood stands on the grass that was under everything.
    assert_eq!(sim.terrain.type_at(20, 6), Cell::Grass);
    assert_eq!(sim.terrain.zone_at(2, 6), Zone::Any);
    assert!(sim.in_water(2, 6), "the map does not agree that it is water");
    assert!(!sim.plant_sim.zones.is_empty(), "the wilderness was not told about the zones");
}

#[test]
fn a_later_layer_wins_and_a_zone_sits_beside_the_ground() {
    // A layer covering everything has no clear pixel to read against, so it
    // is a mask: light everywhere.
    let everywhere = vec![true; 24 * 12];
    let square = drawn(24, 12, |x, y| (8..16).contains(&x) && (4..8).contains(&y));
    let layers = [
        LayerMask { brush: Brush::Sand, w: 24, h: 12, on: &everywhere },
        LayerMask { brush: Brush::Rock, w: 24, h: 12, on: &square },
        LayerMask { brush: Brush::Wood, w: 24, h: 12, on: &square },
        // Says nothing, whatever it covers.
        LayerMask { brush: Brush::Clear, w: 24, h: 12, on: &everywhere },
    ];
    let cells = read_layers(&layers, 24, 12, Cell::Water);
    let at = |c: i32, r: i32| (r * 24 + c) as usize;
    assert_eq!(Cell::from_u8(cells.ground[at(1, 1)]), Cell::Sand, "the sand did not cover the base");
    assert_eq!(Cell::from_u8(cells.ground[at(10, 5)]), Cell::Rock, "the later layer did not win");
    assert_eq!(Zone::from_u8(cells.zone[at(10, 5)]), Zone::Wood, "the zone did not land beside the ground");
    assert_eq!(Zone::from_u8(cells.zone[at(1, 1)]), Zone::Any);
    assert!(cells.sky.iter().all(|&v| v == 0));
}

#[test]
fn a_sky_layer_marks_the_page_and_leaves_the_map_alone() {
    let top = drawn(24, 12, |_, y| y < 3);
    let layers = [LayerMask { brush: Brush::Sky, w: 24, h: 12, on: &top }];
    let cells = read_layers(&layers, 24, 12, Cell::Grass);
    assert_eq!(cells.sky.iter().filter(|&&v| v == 1).count(), 72);
    assert!(cells.ground.iter().all(|&g| Cell::from_u8(g) == Cell::Grass));

    // Laid on a settlement, the marks go nowhere: sky is not land.
    let mut state = State::new();
    state.civ.world.cols = 24;
    state.civ.world.rows = 12;
    let mut sim = Settlement::new(&state);
    lay_cells(&mut sim, &cells);
    assert_eq!(sim.terrain.type_at(1, 1), Cell::Grass);
    assert_eq!(sim.terrain.zone_at(1, 1), Zone::Any);
}

#[test]
fn a_layer_of_another_size_is_stretched_over_the_map() {
    // Eight by four, read as a map of twenty four by twelve: every layer
    // pixel is three cells across.
    let left = drawn(8, 4, |x, _| x < 4);
    let layers = [LayerMask { brush: Brush::Water, w: 8, h: 4, on: &left }];
    let cells = read_layers(&layers, 24, 12, Cell::Grass);
    for r in 0..12 {
        for c in 0..24 {
            let want = if c < 12 { Cell::Water } else { Cell::Grass };
            assert_eq!(Cell::from_u8(cells.ground[(r * 24 + c) as usize]), want, "cell {c},{r}");
        }
    }
}

#[test]
fn cells_read_for_another_size_of_map_are_not_laid() {
    let mut state = State::new();
    state.civ.world.cols = 24;
    state.civ.world.rows = 12;
    let everywhere = vec![true; 8 * 4];
    let layers = [LayerMask { brush: Brush::Water, w: 8, h: 4, on: &everywhere }];
    let cells = read_layers(&layers, 16, 8, Cell::Grass);
    let mut sim = Settlement::new(&state);
    let was = sim.terrain.kind.clone();
    lay_cells(&mut sim, &cells);
    assert_eq!(sim.terrain.kind, was, "cells for a sixteen by eight map were laid on a larger one");
}

#[test]
fn a_town_founded_on_read_layers_keeps_its_water() {
    let mut state = State::new();
    state.civ.world.cols = 40;
    state.civ.world.rows = 20;
    // A lake in the middle, land around it, drawn two pixels to a cell.
    let lake = drawn(80, 40, |x, y| (28..52).contains(&x) && (12..28).contains(&y));
    let layers = [LayerMask { brush: Brush::Water, w: 80, h: 40, on: &lake }];
    let cells = read_layers(&layers, 40, 20, Cell::Grass);
    let mut sim = Settlement::new(&state);
    lay_cells(&mut sim, &cells);
    sim.bootstrap(&state);

    assert!(sim.in_water(20, 10), "the lake was flattened by the founding");
    assert!(!sim.in_water(2, 2), "the shore turned to water");
    // Nobody is standing in the lake, and nothing grew in it.
    for pi in sim.people.live_indices() {
        let (c, r) = (sim.people[pi].cell_col(), sim.people[pi].cell_row());
        assert!(!sim.in_water(c, r) || sim.people[pi].aboard != 0, "somebody was founded in the lake");
    }
}
