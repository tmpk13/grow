//! The map editor: reading a picture in as a map, and what the legend makes
//! of the colors in one.

use grow::civ::map_brush::{lay_cells, read_picture, Brush};
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

// ---- a picture as a map --------------------------------------------------

#[test]
fn every_color_is_read_as_the_nearest_thing_in_the_legend() {
    // Nothing here is exactly a brush color: a drawing is never that tidy.
    assert_eq!(Brush::nearest(pack_rgba(50, 130, 220, 255)), Brush::Water);
    assert_eq!(Brush::nearest(pack_rgba(100, 180, 90, 255)), Brush::Grass);
    assert_eq!(Brush::nearest(pack_rgba(230, 210, 140, 255)), Brush::Sand);
    // Neither of the two that are not ground can ever come out of a picture.
    for v in [0, pack_rgba(255, 255, 255, 255), pack_rgba(0, 0, 0, 255)] {
        let brush = Brush::nearest(v);
        assert!(brush != Brush::Clear && brush != Brush::Sky, "{brush:?} is not ground");
    }
}

#[test]
fn a_picture_laid_over_a_map_becomes_that_map() {
    let mut state = State::new();
    state.civ.world.cols = 24;
    state.civ.world.rows = 12;
    // A picture drawn four pixels to a cell: water in the left half, a wood
    // marked over the right. Two flat halves, which is a picture with no scale
    // to read out of it - what is being checked here is what the colors mean,
    // not how large they were drawn.
    let (w, h, px) = blocks(24, 12, 4, |x, _| {
        if x < 12 {
            Brush::Water.color()
        } else {
            Brush::Wood.color()
        }
    });
    let cells = read_picture(&(w, h, px), 24, 12);
    let mut sim = Settlement::new(&state);
    lay_cells(&mut sim, &cells);

    assert_eq!(sim.terrain.type_at(2, 6), Cell::Water, "the left half is not water");
    assert_eq!(sim.terrain.zone_at(20, 6), Zone::Wood, "the right half was not zoned");
    // A zone says what may take root; it does not turn the ground into
    // anything, and the water half carries no zone at all.
    assert_eq!(sim.terrain.zone_at(2, 6), Zone::Any);
    assert!(sim.in_water(2, 6), "the map does not agree that it is water");
    assert!(!sim.plant_sim.zones.is_empty(), "the wilderness was not told about the zones");
}

#[test]
fn a_town_founded_on_a_read_map_keeps_its_water() {
    let mut state = State::new();
    state.civ.world.cols = 40;
    state.civ.world.rows = 20;
    // A lake in the middle, land around it.
    let (w, h, px) = blocks(40, 20, 2, |x, y| {
        if (14..26).contains(&x) && (6..14).contains(&y) {
            Brush::Water.color()
        } else {
            Brush::Grass.color()
        }
    });
    let cells = read_picture(&(w, h, px), 40, 20);
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
