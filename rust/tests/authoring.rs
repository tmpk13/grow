//! The drawing side of the tool: sheets in the sprite editor, the sizing a
//! dropped sprite comes out at, and how a sampling box is read as a ramp.

use grow::art::{ArtLibrary, Sheet, MAX_SHEET_PX};
use grow::civ::sprites::Clip;
use grow::sampler::{ramp_pick, Materials, ROLES};
use grow::state::State;
use grow::util::{pack_rgba, EMPTY_COLOR};

const RED: u32 = pack_rgba(200, 40, 40, 255);
const BLUE: u32 = pack_rgba(40, 80, 200, 255);

fn sheet_with_two_layers() -> Sheet {
    let mut sheet = Sheet::new("art-test", "Test", 4, 4);
    sheet.add_frame(0, false);
    sheet.add_layer(0, "Top");
    // Bottom layer: a red row across the middle of both frames. Top layer: one
    // blue pixel over it in the first frame only.
    for f in 0..2 {
        for x in 0..4 {
            sheet.set(0, f, x, 2, RED);
        }
    }
    sheet.set(1, 0, 1, 2, BLUE);
    sheet
}

// ---- sheets --------------------------------------------------------------

#[test]
fn a_layer_higher_in_the_stack_covers_the_one_below() {
    let sheet = sheet_with_two_layers();
    let first = sheet.flatten(0);
    // The drawn row is y = 2 in a 4 wide sheet, so the row starts at index 8.
    assert_eq!(first[8 + 1], BLUE, "the upper layer did not show");
    assert_eq!(first[8], RED, "the lower layer was lost");
    assert_eq!(first[0], EMPTY_COLOR, "something was drawn where nothing is");
    // The second frame has nothing on the upper layer, so it is all red.
    assert_eq!(sheet.flatten(1)[8 + 1], RED);
}

#[test]
fn a_hidden_layer_is_kept_but_not_drawn() {
    let mut sheet = sheet_with_two_layers();
    sheet.layers[1].visible = false;
    assert_eq!(sheet.flatten(0)[8 + 1], RED, "a hidden layer still drew");
    assert_eq!(sheet.get(1, 0, 1, 2), BLUE, "hiding a layer threw its pixels away");
}

#[test]
fn merging_a_layer_down_keeps_whichever_pixel_was_on_top() {
    let mut sheet = sheet_with_two_layers();
    let landed = sheet.merge_down(1);
    assert_eq!(landed, 0, "the selection did not follow the merge");
    assert_eq!(sheet.layers.len(), 1);
    assert_eq!(sheet.get(0, 0, 1, 2), BLUE, "the upper pixel lost to the lower one");
    assert_eq!(sheet.get(0, 0, 0, 2), RED, "the lower pixel was wiped");
    assert_eq!(sheet.get(0, 1, 1, 2), RED, "the merge reached a frame it was not on");
}

#[test]
fn frames_are_added_and_removed_across_every_layer_at_once() {
    let mut sheet = sheet_with_two_layers();
    assert_eq!(sheet.frame_count(), 2);
    // A duplicate of the first frame lands after it, on both layers.
    let at = sheet.add_frame(0, true);
    assert_eq!(at, 1);
    assert_eq!(sheet.frame_count(), 3);
    for layer in &sheet.layers {
        assert_eq!(layer.cels.len(), 3, "a layer was left behind");
    }
    assert_eq!(sheet.get(1, 1, 1, 2), BLUE, "the duplicate lost the upper layer");

    let at = sheet.remove_frame(1);
    assert_eq!(at, 0);
    assert_eq!(sheet.frame_count(), 2);
    for layer in &sheet.layers {
        assert_eq!(layer.cels.len(), 2);
    }
    // The last frame is never removed, however often it is asked for.
    let mut single = Sheet::new("one", "One", 2, 2);
    single.remove_frame(0);
    assert_eq!(single.frame_count(), 1);
}

#[test]
fn resizing_a_sheet_keeps_the_art_rather_than_resampling_it() {
    let mut sheet = sheet_with_two_layers();
    sheet.resize(8, 8);
    assert_eq!((sheet.w, sheet.h), (8, 8));
    assert_eq!(sheet.get(0, 0, 3, 2), RED, "the art moved when the sheet grew");
    assert_eq!(sheet.get(0, 0, 4, 2), EMPTY_COLOR, "the new room was not empty");
    sheet.resize(2, 2);
    assert_eq!(sheet.get(0, 0, 1, 1), EMPTY_COLOR, "the crop kept a row it lost");
}

#[test]
fn a_sheet_survives_a_project_file() {
    let mut state = State::new();
    state.art = ArtLibrary { sheets: vec![sheet_with_two_layers()] };
    let back = State::from_json(&state.to_json()).expect("round trip");
    let sheet = back.art.find("art-test").expect("the sheet came back");
    assert_eq!(sheet.layers.len(), 2);
    assert_eq!(sheet.frame_count(), 2);
    assert_eq!(sheet.get(1, 0, 1, 2), BLUE);
    assert_eq!(sheet.get(0, 1, 3, 2), RED);
    assert_eq!(sheet.get(0, 0, 0, 0), EMPTY_COLOR);
}

#[test]
fn a_sheet_of_nothing_is_not_offered_as_person_art() {
    let blank = Sheet::new("blank", "Blank", 4, 4);
    assert!(!blank.any());
    assert!(Clip::from_sheet(&blank).is_none());

    let sheet = sheet_with_two_layers();
    let clip = Clip::from_sheet(&sheet).expect("a drawn sheet makes a clip");
    assert_eq!(clip.frame_count(), 2);
    // The frame is the canvas it was drawn on, empty rows and all, and it reads
    // the flattened stack rather than any one layer.
    assert_eq!((clip.frame_w(), clip.h), (4, 4));
    assert_eq!(clip.pixel(0, 1, 2), BLUE);
    assert_eq!(clip.pixel(1, 3, 2), RED);
    assert_eq!(clip.pixel(0, 0, 0), 0, "an empty row of the canvas was dropped");
}

#[test]
fn pixels_saved_before_the_run_encoding_still_read_back() {
    // Cels were runs of a two digit count and a color; clips were one color per
    // pixel with nothing in front of them. Both still load, and both are
    // written back out tagged, which is what makes the wider run safe to use.
    use grow::util::packed_to_rgba_hex as hex;
    let red = grow::util::pack_rgba(255, 0, 0, 255);
    let blue = grow::util::pack_rgba(0, 0, 255, 255);

    let narrow = format!("02{}01{}", hex(red), hex(blue));
    let cel: grow::art::Cel =
        serde_json::from_str(&format!(r#"{{"px":"{narrow}"}}"#)).expect("a cel");
    assert_eq!(cel.px, vec![red, red, blue]);

    let plain = format!("{}{}{}", hex(red), hex(red), hex(blue));
    let clip: Clip = serde_json::from_str(&format!(
        r#"{{"w":3,"h":1,"px":"{plain}","frames":1}}"#
    ))
    .expect("a clip");
    assert_eq!(clip.px, vec![red, red, blue]);
    assert_eq!(clip.pixel(0, 2, 0), blue);

    let out = serde_json::to_string(&clip).expect("writes");
    assert!(out.contains(r#""px":"r"#), "the clip was not written as runs: {out}");
    let back: Clip = serde_json::from_str(&out).expect("reads its own writing");
    assert_eq!(back.px, clip.px);
}

#[test]
fn a_blank_frame_costs_almost_nothing_to_save() {
    // The cap on a frame is two hundred and fifty six pixels a side, which is
    // sixty five thousand of them. Whatever a project spends on that, it cannot
    // be a character each.
    let sheet = Sheet::new("big", "Big", MAX_SHEET_PX, MAX_SHEET_PX);
    assert!(sheet.bytes() < 64, "an empty sheet costs {} characters", sheet.bytes());
    let clip = Clip::from_strip(
        MAX_SHEET_PX,
        MAX_SHEET_PX,
        vec![0u32; (MAX_SHEET_PX * MAX_SHEET_PX) as usize],
        1,
        "blank".into(),
    )
    .expect("a strip");
    assert!(clip.bytes() < 64, "an empty clip costs {} characters", clip.bytes());
}

// ---- what a dropped sprite comes out sized at ----------------------------

/// A strip of `frames` frames of `fw` by `fh`, with a solid box of `art_w` by
/// `art_h` sitting `pad` in from the top left of every frame.
fn padded_strip(frames: i32, fw: i32, fh: i32, art_w: i32, art_h: i32, pad: i32) -> Clip {
    let w = fw * frames;
    let mut px = vec![0u32; (w * fh) as usize];
    for f in 0..frames {
        for y in 0..art_h {
            for x in 0..art_w {
                px[((pad + y) * w + f * fw + pad + x) as usize] = RED;
            }
        }
    }
    Clip::from_strip(w, fh, px, frames, "padded".into()).expect("clip")
}

#[test]
fn a_dropped_sprite_keeps_the_frame_it_was_drawn_in() {
    // Where the art sits in the frame is the composition: it is what puts a
    // figure's feet on the ground and holds a tool out to the side of them, and
    // it is what makes two motions exported from one canvas line up.
    let tight = padded_strip(2, 6, 8, 4, 6, 1);
    let loose = padded_strip(2, 32, 40, 4, 6, 9);
    assert_eq!((tight.frame_w(), tight.h), (6, 8));
    assert_eq!((loose.frame_w(), loose.h), (32, 40));
    assert_eq!(tight.pixel(0, 1, 1), RED, "the art moved inside the frame");
    assert_eq!(loose.pixel(0, 9, 9), RED, "the art moved inside the frame");
    assert_eq!(tight.pixel(0, 0, 0), 0, "the padding was cropped away");
    assert_eq!(loose.pixel(1, 0, 0), 0, "the padding was cropped away");
}

#[test]
fn how_large_a_sprite_comes_out_is_the_size_of_the_image() {
    // A source pixel is worth a fixed fraction of a cell, so the same art is
    // the same size in every slot it is dropped on, and however it was padded.
    let tight = padded_strip(2, 6, 8, 4, 6, 1);
    let loose = padded_strip(2, 32, 40, 4, 6, 9);
    // Drawn at the resolution it was authored at, a pixel is a pixel.
    assert_eq!(tight.drawn_size(8, 8.0), (6, 8));
    assert_eq!(loose.drawn_size(8, 8.0), (32, 40));
    // Half the resolution is twice the size, and the padding scales with the
    // art rather than being measured against it.
    assert_eq!(tight.drawn_size(8, 4.0), (12, 16));
    assert_eq!(loose.drawn_size(8, 4.0), (64, 80));
    // A wider cell is a larger world, and the art grows with it.
    assert_eq!(tight.drawn_size(16, 8.0), (12, 16));
}

#[test]
fn a_picture_is_never_squashed_to_fit_what_it_stands_for() {
    // Three by nine is three by nine at any resolution, at any cell width and
    // at any scale: both sides are held to one ratio.
    let (w, h) = (3, 9);
    let mut clip = padded_strip(1, w, h, w, h, 0);
    for cell in [4, 8, 11, 24] {
        for per_cell in [1.0, 5.0, 8.0, 32.0] {
            for scale in [0.25, 1.0, 3.0] {
                clip.scale = scale;
                let (dw, dh) = clip.drawn_size(cell, per_cell);
                assert!(dh >= dw, "the taller side came out the shorter one");
                // Below a couple of pixels there is nothing left to hold a
                // ratio in: a pixel is the smallest a side can be.
                if dw >= 2 {
                    let want = dw as f64 * h as f64 / w as f64;
                    assert!(
                        (dh as f64 - want).abs() <= 1.0,
                        "{dw}x{dh} is not three by nine at cell {cell}, {per_cell} per cell, scale {scale}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_sheet_that_does_not_divide_evenly_does_not_drift() {
    // Ten columns read as four frames. The frames are two wide with two
    // columns over; stepping by the floored width would put the last frame two
    // columns before where it belongs.
    let (w, h, frames) = (10, 2, 4);
    let mut px = vec![0u32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            px[(y * w + x) as usize] = (x + 1) as u32;
        }
    }
    let clip = Clip::from_strip(w, h, px, frames, "uneven".into()).expect("clip");
    assert_eq!(clip.frame_w(), 2);
    let starts: Vec<u32> = (0..frames).map(|f| clip.pixel(f, 0, 0)).collect();
    assert_eq!(starts, vec![1, 3, 6, 8], "the frames are cut off true");
}

#[test]
fn mirroring_a_clip_reads_every_frame_backwards() {
    let (w, h, frames) = (6, 1, 2);
    let px: Vec<u32> = (0..w * h).map(|i| (i + 1) as u32).collect();
    let mut clip = Clip::from_strip(w, h, px, frames, "mirror".into()).expect("clip");
    assert_eq!(clip.pixel(0, 0, 0), 1);
    assert_eq!(clip.pixel(1, 0, 0), 4);
    clip.mirror = true;
    assert_eq!(clip.pixel(0, 0, 0), 3, "the first frame did not turn around");
    assert_eq!(clip.pixel(0, 2, 0), 1);
    assert_eq!(clip.pixel(1, 0, 0), 6, "the second frame read the first one");
    // Turning it off gives the sheet back rather than a sheet turned twice.
    clip.mirror = false;
    assert_eq!(clip.pixel(0, 0, 0), 1);
}

// ---- what stands behind the map ------------------------------------------

fn hill(x: f64, distance: f64) -> grow::civ::scenery::Scene {
    grow::civ::scenery::Scene {
        shape: grow::civ::scenery::Shape::Bank,
        x,
        width: 20.0,
        height: 8.0,
        distance,
        snow: 1.0,
        sampler: "mat-stone".to_string(),
        seed: 5,
    }
}

#[test]
fn a_piece_of_scenery_covers_its_own_span_and_nothing_else() {
    use grow::civ::scenery::at;
    let world = grow::world::World::new(&grow::civ::config::default_civ_world());
    let cell = world.cell_px as f64;
    let piece = hill(30.0, 0.4);
    // Its outline never leaves nought to one, so nothing is ever drawn below
    // the horizon or above the height it was given.
    for i in 0..=100 {
        let t = i as f64 / 100.0;
        let v = piece.profile(t);
        assert!((0.0..=1.0).contains(&v), "the outline reads {v} at {t}");
    }
    assert_eq!(piece.profile(-0.01), 0.0);
    assert_eq!(piece.profile(1.01), 0.0);

    let scenery = vec![piece];
    // The middle of it, just under the horizon: that is the piece.
    let mid_x = (30.0 * cell) as i32;
    assert_eq!(at(&scenery, &world, mid_x, world.sky_px - 1), Some(0));
    // Well to the side of it is open sky, and so is anything below the
    // horizon: the land is not the sky.
    assert_eq!(at(&scenery, &world, mid_x + (40.0 * cell) as i32, world.sky_px - 1), None);
    assert_eq!(at(&scenery, &world, mid_x, world.sky_px + 4), None);
    assert_eq!(at(&scenery, &world, mid_x, 0), None, "the sky above its summit is not it");
}

#[test]
fn the_furthest_thing_is_drawn_first_and_pressed_last() {
    use grow::civ::scenery::{at, back_to_front};
    let world = grow::world::World::new(&grow::civ::config::default_civ_world());
    let cell = world.cell_px as f64;
    // Two hills in the same place, one behind the other.
    let scenery = vec![hill(30.0, 0.2), hill(30.0, 0.9)];
    assert_eq!(back_to_front(&scenery), vec![1, 0], "the far one is not drawn first");
    // A press lands on the near one, which is what is on top of the picture.
    let mid_x = (30.0 * cell) as i32;
    assert_eq!(at(&scenery, &world, mid_x, world.sky_px - 1), Some(0));
}

#[test]
fn scenery_is_part_of_the_project_file() {
    let mut state = State::new();
    state.civ.scenery = vec![hill(12.0, 0.35)];
    state.civ.scenery[0].snow = 0.6;
    let back = State::from_json(&state.to_json()).expect("round trip");
    assert_eq!(back.civ.scenery.len(), 1);
    let piece = &back.civ.scenery[0];
    assert_eq!(piece.shape, grow::civ::scenery::Shape::Bank);
    assert_eq!(piece.x, 12.0);
    assert_eq!(piece.snow, 0.6);
    assert_eq!(piece.sampler, "mat-stone");
}

// ---- how a sampling box is read ------------------------------------------

fn box_of(colors: &[(u32, usize)]) -> (Materials, String) {
    let mut materials = Materials::new();
    let id = materials.samplers[0].id.clone();
    {
        let sampler = materials.find_mut(&id).expect("box");
        let mut at = 0;
        for (color, count) in colors {
            for _ in 0..*count {
                if at < sampler.px.len() {
                    sampler.px[at] = *color;
                    at += 1;
                }
            }
        }
        // Whatever is left of the box takes the last color listed, so a case
        // reads as the handful of pixels it cares about and then the ground
        // they sit on.
        let rest = colors[colors.len() - 1].0;
        while at < sampler.px.len() {
            sampler.px[at] = rest;
            at += 1;
        }
    }
    materials.touch();
    (materials, id)
}

#[test]
fn a_tone_holds_as_much_of_the_ramp_as_it_covers_of_the_box() {
    let light = pack_rgba(240, 240, 240, 255);
    let dark = pack_rgba(20, 20, 20, 255);
    let (materials, id) = box_of(&[(light, 1), (dark, 0)]);
    let ramp = materials.ramp(&id);
    assert_eq!(ramp.len(), 2, "the palette is still one entry per color");
    assert_eq!(ramp[0], dark, "the palette is not dark to light");

    let lut = materials.tone_lut(&id);
    let bright = lut.iter().filter(|c| **c == light).count();
    assert!(bright >= 1, "a color in the box fell out of the ramp");
    assert!(
        bright * 8 < lut.len(),
        "one pixel of highlight took {bright} of {} steps",
        lut.len()
    );
    // The middle of a box that is nearly all one color is that color; an even
    // spread over the distinct colors would answer with the highlight.
    assert_eq!(ramp_pick(&lut, 0.5), dark);
    assert_eq!(ramp_pick(&lut, 1.0), light, "the lightest tone left the top");
    assert_eq!(ramp_pick(&lut, 0.0), dark);
}

#[test]
fn every_color_in_the_box_reaches_the_ramp_however_little_of_it_there_is() {
    let rare: Vec<(u32, usize)> = (0..6)
        .map(|i| (pack_rgba(30 + i * 40, 30 + i * 40, 30 + i * 40, 255), 1))
        .collect();
    let (materials, id) = box_of(&rare);
    let lut = materials.tone_lut(&id);
    for (color, _) in &rare {
        assert!(
            lut.contains(color),
            "a color in the box got no share of the ramp"
        );
    }
    // Order survives the weighting: the ramp still runs dark to light.
    let mut sorted = lut.to_vec();
    sorted.dedup();
    let ramp = materials.ramp(&id);
    assert_eq!(sorted, *ramp, "the weighted ramp is out of order");
}

#[test]
fn an_empty_box_reads_as_no_ramp_at_all() {
    let mut materials = Materials::new();
    let id = materials.samplers[0].id.clone();
    materials.find_mut(&id).expect("box").px.fill(EMPTY_COLOR);
    materials.touch();
    assert!(materials.ramp(&id).is_empty());
    assert!(materials.tone_lut(&id).is_empty());
    assert_eq!(ramp_pick(&materials.tone_lut(&id), 0.5), EMPTY_COLOR);
}

// ---- stepping back -------------------------------------------------------

/// Undo works against the project directly rather than through the shell,
/// which is what makes it testable off a browser.
fn state_with_a_sheet() -> State {
    let mut state = State::new();
    state.art = ArtLibrary { sheets: vec![sheet_with_two_layers()] };
    state
}

#[test]
fn a_stroke_on_a_sheet_can_be_put_back() {
    use grow::undo::History;
    let mut state = state_with_a_sheet();
    let mut history = History::default();
    assert!(!history.can_undo());

    history.record(&state, "stroke", false, 0.0);
    state.art.find_mut("art-test").expect("sheet").set(0, 0, 0, 0, BLUE);
    assert_eq!(state.art.find("art-test").expect("sheet").get(0, 0, 0, 0), BLUE);

    assert!(history.can_undo());
    assert!(history.undo(&mut state));
    assert_eq!(
        state.art.find("art-test").expect("sheet").get(0, 0, 0, 0),
        EMPTY_COLOR,
        "the stroke did not come off"
    );
    assert!(history.can_redo());
    assert!(history.redo(&mut state));
    let sheet = state.art.find("art-test").expect("sheet");
    assert_eq!(sheet.get(0, 0, 0, 0), BLUE);
    // Neither layer lost anything it had before the step either way.
    assert_eq!(sheet.get(0, 0, 1, 2), RED, "the lower layer moved with the step");
    assert_eq!(sheet.get(1, 0, 1, 2), BLUE, "the upper layer moved with the step");
}

#[test]
fn a_step_covers_everything_a_project_holds() {
    use grow::undo::History;
    let mut state = state_with_a_sheet();
    let mut history = History::default();
    let box_id = state.materials.samplers[0].id.clone();

    history.record(&state, "an edit", false, 0.0);
    state.materials.find_mut(&box_id).expect("box").px[0] = RED;
    state.species[0].name = "Renamed".into();
    state.civ.people.walk_speed = 9.5;
    state.world.cols = 111;
    state.art.sheets.push(Sheet::new("art-new", "New", 8, 8));

    // One step back takes all five, because a step is the project rather than
    // whichever buffer happened to be edited.
    assert!(history.undo(&mut state));
    assert_ne!(state.materials.find(&box_id).expect("box").px[0], RED);
    assert_eq!(state.species[0].name, State::new().species[0].name);
    assert_eq!(state.civ.people.walk_speed, State::new().civ.people.walk_speed);
    assert_eq!(state.world.cols, State::new().world.cols);
    assert_eq!(state.art.sheets.len(), 1);
}

#[test]
fn a_control_being_held_makes_one_step_rather_than_one_a_frame() {
    use grow::undo::History;
    let mut state = State::new();
    let mut history = History::default();
    let start = state.civ.people.walk_speed;

    // A slider drag: the same control, over and over, inside the window.
    for (i, v) in [1.0, 2.0, 3.0, 4.0].iter().enumerate() {
        history.record(&state, "Walking speed", true, i as f64 * 100.0);
        state.civ.people.walk_speed = *v;
    }
    assert!(history.undo(&mut state));
    assert_eq!(state.civ.people.walk_speed, start, "the whole drag was not one step");
    assert!(!history.can_undo(), "the drag left more than one step behind");

    // A different control never joins a step that is already there, however
    // close together the two are.
    history.record(&state, "Walking speed", true, 1000.0);
    state.civ.people.walk_speed = 5.0;
    history.record(&state, "Path speed bonus", true, 1010.0);
    state.civ.people.road_speed_bonus = 1.0;
    history.undo(&mut state);
    assert_eq!(state.civ.people.walk_speed, 5.0, "two controls were merged into one step");

    // Nor does a control somebody presses rather than holds.
    history.clear();
    history.record(&state, "Add box", false, 2000.0);
    state.materials.samplers.push(state.materials.samplers[0].clone());
    history.record(&state, "Add box", false, 2010.0);
    state.materials.samplers.push(state.materials.samplers[0].clone());
    history.undo(&mut state);
    assert_eq!(state.materials.samplers.len(), ROLES.len() + 1, "two presses became one step");
}

#[test]
fn a_new_edit_drops_whatever_had_been_undone() {
    use grow::undo::History;
    let mut state = state_with_a_sheet();
    let mut history = History::default();

    history.record(&state, "stroke", false, 0.0);
    state.art.find_mut("art-test").expect("sheet").set(0, 0, 0, 0, BLUE);
    history.undo(&mut state);
    assert!(history.can_redo());

    // A step forward from here would put back something this branch never had.
    history.record(&state, "stroke", false, 5000.0);
    assert!(!history.can_redo(), "the abandoned branch is still on the stack");
}

#[test]
fn stepping_back_with_nothing_recorded_does_nothing() {
    use grow::undo::History;
    let mut state = State::new();
    let mut history = History::default();
    assert!(!history.undo(&mut state));
    assert!(!history.redo(&mut state));
}

// ---- images into a sheet -------------------------------------------------

#[test]
fn a_dropped_image_is_fitted_to_the_frame_and_centered() {
    let mut sheet = Sheet::new("drop", "Drop", 8, 8);
    // Wider than it is tall, and larger than the frame either way: it should
    // come back scaled to the width and sat in the middle of the height.
    let (sw, sh) = (16, 8);
    let px = vec![RED; (sw * sh) as usize];
    sheet.place(0, 0, sw, sh, &px);
    assert_eq!(sheet.get(0, 0, 0, 2), RED, "the image did not reach the left edge");
    assert_eq!(sheet.get(0, 0, 7, 2), RED, "the image did not reach the right edge");
    assert_eq!(sheet.get(0, 0, 0, 0), EMPTY_COLOR, "the image was stretched to the frame");
    assert_eq!(sheet.get(0, 0, 0, 7), EMPTY_COLOR);

    // Smaller than the frame is left at its own size rather than blown up.
    let mut small = Sheet::new("small", "Small", 8, 8);
    small.place(0, 0, 2, 2, &[BLUE; 4]);
    let painted = (0..8)
        .flat_map(|y| (0..8).map(move |x| (x, y)))
        .filter(|(x, y)| small.get(0, 0, *x, *y) != EMPTY_COLOR)
        .count();
    assert_eq!(painted, 4, "a small image was scaled up");
    assert_eq!(small.get(0, 0, 3, 3), BLUE, "a small image was not centered");
}

#[test]
fn dropping_onto_a_layer_replaces_only_that_layer() {
    let mut sheet = sheet_with_two_layers();
    // A corner the 2x2 image will not reach, so what happens to it says whether
    // the cel was cleared rather than drawn over.
    sheet.set(1, 0, 0, 0, BLUE);
    sheet.place(1, 0, 2, 2, &[RED; 4]);
    assert_eq!(sheet.get(1, 0, 0, 0), EMPTY_COLOR, "the cel was not cleared first");
    assert_eq!(sheet.get(1, 0, 1, 1), RED, "the image did not land");
    assert_eq!(sheet.get(0, 0, 0, 2), RED, "the layer below lost its own pixels");
    assert_eq!(sheet.get(1, 1, 1, 2), EMPTY_COLOR, "another frame was touched");
}

#[test]
fn nudging_moves_the_art_and_drops_what_leaves_the_frame() {
    let mut sheet = sheet_with_two_layers();
    sheet.shift_cel(0, 0, 1, 0);
    assert_eq!(sheet.get(0, 0, 0, 2), EMPTY_COLOR, "the row did not move");
    assert_eq!(sheet.get(0, 0, 3, 2), RED);
    // The pixel that was at the right edge has nowhere to go and is gone; a
    // nudge back does not bring it with it.
    sheet.shift_cel(0, 0, -1, 0);
    assert_eq!(sheet.get(0, 0, 3, 2), EMPTY_COLOR, "a pixel came back off the edge");
    assert_eq!(sheet.get(0, 0, 0, 2), RED);
    // Only the cel that was asked for moved.
    assert_eq!(sheet.get(1, 0, 1, 2), BLUE, "another layer moved with it");
    assert_eq!(sheet.get(0, 1, 3, 2), RED, "another frame moved with it");

    sheet.shift_all(0, -1);
    assert_eq!(sheet.get(0, 0, 0, 1), RED, "the whole sheet did not move");
    assert_eq!(sheet.get(1, 0, 1, 1), BLUE);
    assert_eq!(sheet.get(0, 1, 3, 1), RED);
}

#[test]
fn a_clip_remembers_which_sheet_it_was_built_from() {
    let sheet = sheet_with_two_layers();
    let clip = Clip::from_sheet(&sheet).expect("clip");
    assert_eq!(clip.sheet, "art-test");
    // A dropped image has no sheet behind it, and nothing should invent one.
    let dropped = Clip::from_strip(4, 2, vec![RED; 8], 2, "walk.png".into()).expect("clip");
    assert!(dropped.sheet.is_empty());
}

// ---- where in the box a color was drawn ----------------------------------

/// A box with one color across its top rows and another across its bottom
/// rows, so which of the two comes back says which part of the box was read.
fn split_box(top: u32, bottom: u32) -> (Materials, String) {
    let mut materials = Materials::new();
    let id = materials.samplers[0].id.clone();
    {
        let sampler = materials.find_mut(&id).expect("box");
        let (w, h) = (sampler.w, sampler.h);
        for y in 0..h {
            let c = if y < h / 2 { top } else { bottom };
            for x in 0..w {
                sampler.px[(y * w + x) as usize] = c;
            }
        }
    }
    materials.touch();
    (materials, id)
}

#[test]
fn the_top_of_a_box_draws_the_top_of_the_object() {
    let pale = pack_rgba(230, 230, 230, 255);
    let dark = pack_rgba(30, 30, 30, 255);
    let (materials, id) = split_box(pale, dark);
    let bands = materials.bands(&id);

    // Whatever the tone asked for, the top of the object only has the color
    // from the top of the box to give, and the foot only the one from the foot.
    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        assert_eq!(bands.pick(t, 0.0), pale, "the top of the object read the wrong end");
        assert_eq!(bands.pick(t, 1.0), dark, "the foot of the object read the wrong end");
    }
    // The other way round in the box is the other way round on the object, so
    // this is the arrangement being read and not the luminance.
    let (flipped, id) = split_box(dark, pale);
    let bands = flipped.bands(&id);
    assert_eq!(bands.pick(0.5, 0.0), dark);
    assert_eq!(bands.pick(0.5, 1.0), pale);
}

#[test]
fn a_box_drawn_the_same_all_the_way_down_reads_the_same_all_the_way_down() {
    // A box whose rows are identical - tones across it rather than down it -
    // has nothing to say about height, and must read the same at every one of
    // them. This is what keeps the arrangement mattering from changing what a
    // box that ignores it does.
    let mut materials = Materials::new();
    let id = materials.samplers[0].id.clone();
    {
        let sampler = materials.find_mut(&id).expect("box");
        let (w, h) = (sampler.w, sampler.h);
        for y in 0..h {
            for x in 0..w {
                let step = x * 5 / w;
                sampler.px[(y * w + x) as usize] =
                    pack_rgba(step * 50, step * 50, step * 50, 255);
            }
        }
    }
    materials.touch();
    let bands = materials.bands(&id);
    for t in [0.0, 0.3, 0.6, 1.0] {
        let at_top = bands.pick(t, 0.0);
        for v in [0.25, 0.5, 0.75, 1.0] {
            assert_eq!(bands.pick(t, v), at_top, "an even box read differently at {v}");
        }
    }
}

#[test]
fn the_middle_of_a_box_reaches_most_of_the_way_either_way() {
    // A band is not a slice: what is drawn in the middle of the box has to
    // reach most of the object, or every box would read as three flat bands.
    let mut materials = Materials::new();
    let id = materials.samplers[0].id.clone();
    let mid = pack_rgba(120, 200, 120, 255);
    let edge = pack_rgba(20, 20, 20, 255);
    {
        let sampler = materials.find_mut(&id).expect("box");
        let (w, h) = (sampler.w, sampler.h);
        for y in 0..h {
            let c = if y == h / 2 { mid } else { edge };
            for x in 0..w {
                sampler.px[(y * w + x) as usize] = c;
            }
        }
    }
    materials.touch();
    let bands = materials.bands(&id);
    let reach = [0.0, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .filter(|v| (0..grow::sampler::TONE_STEPS).any(|i| {
            bands.pick(i as f64 / grow::sampler::TONE_STEPS as f64, **v) == mid
        }))
        .count();
    assert!(reach >= 4, "a color drawn mid box only reached {reach} of five heights");
}

#[test]
fn an_empty_box_has_no_bands_to_read() {
    let mut materials = Materials::new();
    let id = materials.samplers[0].id.clone();
    materials.find_mut(&id).expect("box").px.fill(EMPTY_COLOR);
    materials.touch();
    let bands = materials.bands(&id);
    assert!(bands.is_empty());
    assert_eq!(bands.pick(0.5, 0.5), EMPTY_COLOR);
}

// ---- what is drawn in front of what --------------------------------------

#[test]
fn a_contact_shadow_stops_at_the_horizon() {
    use grow::sim::cast_shadow;
    use grow::world::{World, WorldConfig};
    let world = World::new(&WorldConfig { cols: 16, rows: 8, sky_px: 40, ..WorldConfig::default() });
    let mut buf = vec![RED; (world.px_w * world.px_h) as usize];

    // A plant standing in the back row, wide enough that its ellipse would
    // reach above the horizon if nothing stopped it. The shadow follows the
    // drawn box rather than the bare radius - a dying plant is eaten from the
    // tips down and stops shading ground its crown no longer covers - so the
    // box has to say something that wide is actually drawn.
    let state = State::new();
    let species = &state.species[0];
    let limits = grow::species::effective_limits(species, &state.class_limits);
    let mut plant =
        grow::plant::Plant::new(1, species, limits, 8, 0, &world, grow::rng::Rng::new(5));
    plant.radius_px = 30.0;
    plant.bounds.include(plant.ox - 30, 0);
    plant.bounds.include(plant.ox + 30, 1);
    cast_shadow(&world, &mut buf, world.anchor_x(8), world.sky_px + 1, &plant);

    let untouched = (0..world.sky_px)
        .flat_map(|y| (0..world.px_w).map(move |x| (x, y)))
        .all(|(x, y)| buf[(y * world.px_w + x) as usize] == RED);
    assert!(untouched, "a shadow reached into the sky");
    // It did land somewhere, or the test proves nothing.
    assert!(
        buf.iter().any(|c| *c != RED),
        "the shadow did not land at all, so the clamp is not what stopped it"
    );
}


// ---- a sheet that has moved on ------------------------------------------

#[test]
fn a_clip_knows_whether_its_sheet_has_been_drawn_on_since() {
    use grow::civ::sprites::{Clip, FromSheet};

    let mut sheet = Sheet::new("art-1", "Walk", 8, 8);
    sheet.set(0, 0, 3, 3, 0xff00ff00);
    let clip = Clip::from_sheet(&sheet).expect("something is drawn on it");
    assert_eq!(clip.sheet, "art-1");
    assert_eq!(clip.against(Some(&sheet)), FromSheet::Current);

    // One pixel is enough: the people on the map are no longer showing it.
    sheet.set(0, 0, 4, 4, 0xff0000ff);
    assert_eq!(clip.against(Some(&sheet)), FromSheet::Behind);

    // Put back the way it was, and it is current again.
    sheet.set(0, 0, 4, 4, 0);
    assert_eq!(clip.against(Some(&sheet)), FromSheet::Current);

    assert_eq!(clip.against(None), FromSheet::Gone);
}

#[test]
fn a_dropped_clip_has_no_sheet_to_be_behind() {
    use grow::civ::sprites::{Clip, FromSheet};

    let px = vec![0xff00ff00u32; 8 * 8];
    let clip = Clip::from_strip(8, 8, px, 1, "walk.png".into()).expect("a strip");
    assert!(clip.sheet.is_empty());
    assert_eq!(clip.against(None), FromSheet::Dropped);
}

#[test]
fn the_frame_rate_and_the_layers_are_part_of_the_stamp() {
    let mut sheet = Sheet::new("art-1", "Walk", 8, 8);
    sheet.set(0, 0, 3, 3, 0xff00ff00);
    let was = sheet.stamp();

    sheet.fps += 1.0;
    assert_ne!(sheet.stamp(), was, "the rate a clip is built with is part of it");
    sheet.fps -= 1.0;
    assert_eq!(sheet.stamp(), was);

    sheet.layers[0].visible = !sheet.layers[0].visible;
    assert_ne!(sheet.stamp(), was, "a hidden layer changes what a clip would hold");
}

// ---- reordering by dragging ---------------------------------------------

/// A sheet whose frames can be told apart: frame n has the value n+1 in its
/// top left pixel.
fn numbered_frames(n: i32) -> Sheet {
    let mut sheet = Sheet::new("art-1", "Walk", 4, 4);
    for f in 0..n {
        if f > 0 {
            sheet.add_frame(f - 1, false);
        }
    }
    for f in 0..n {
        sheet.set(0, f, 0, 0, (f + 1) as u32);
    }
    sheet
}

fn frame_marks(sheet: &Sheet) -> Vec<u32> {
    (0..sheet.frame_count()).map(|f| sheet.get(0, f, 0, 0)).collect()
}

#[test]
fn a_frame_dragged_along_walks_past_the_others_rather_than_swapping() {
    let mut sheet = numbered_frames(4);
    assert_eq!(frame_marks(&sheet), vec![1, 2, 3, 4]);

    // The first frame dropped at the end is last, and the rest close up.
    assert_eq!(sheet.drag_frame(0, 3), 3);
    assert_eq!(frame_marks(&sheet), vec![2, 3, 4, 1]);

    // And back again.
    assert_eq!(sheet.drag_frame(3, 0), 0);
    assert_eq!(frame_marks(&sheet), vec![1, 2, 3, 4]);
}

#[test]
fn dragging_a_frame_onto_itself_changes_nothing() {
    let mut sheet = numbered_frames(3);
    assert_eq!(sheet.drag_frame(1, 1), 1);
    assert_eq!(frame_marks(&sheet), vec![1, 2, 3]);
}

#[test]
fn a_frame_dropped_off_the_end_lands_on_the_end() {
    let mut sheet = numbered_frames(3);
    assert_eq!(sheet.drag_frame(0, 99), 2);
    assert_eq!(frame_marks(&sheet), vec![2, 3, 1]);
    assert_eq!(sheet.frame_count(), 3, "nothing should have been added or lost");
}

#[test]
fn a_layer_dragged_along_walks_past_the_others() {
    let mut sheet = Sheet::new("art-1", "Walk", 4, 4);
    sheet.add_layer(0, "Two");
    sheet.add_layer(1, "Three");
    let names: Vec<String> = sheet.layers.iter().map(|l| l.name.clone()).collect();
    assert_eq!(names.len(), 3);

    assert_eq!(sheet.drag_layer(2, 0), 0);
    let after: Vec<String> = sheet.layers.iter().map(|l| l.name.clone()).collect();
    assert_eq!(after[0], names[2]);
    assert_eq!(after[1], names[0]);
    assert_eq!(after[2], names[1]);
}

#[test]
fn dragging_in_a_sheet_with_one_of_a_thing_does_nothing() {
    let mut sheet = Sheet::new("art-1", "Walk", 4, 4);
    assert_eq!(sheet.frame_count(), 1);
    assert_eq!(sheet.drag_frame(0, 5), 0);
    assert_eq!(sheet.layers.len(), 1);
    assert_eq!(sheet.drag_layer(0, 3), 0);
}


// ---- names -------------------------------------------------------------

#[test]
fn a_label_slugs_to_something_a_selector_can_hold() {
    use grow::util::slug;
    assert_eq!(slug("Cell depth (px)"), "cell-depth-px");
    assert_eq!(slug("Walls and gates"), "walls-and-gates");
    assert_eq!(slug("Seed"), "seed");
    assert_eq!(slug("  spaced  out  "), "spaced-out");
    assert_eq!(slug("!!!"), "");
    for label in ["Rows (depth)", "Frames", "Pick"] {
        assert!(
            slug(label).chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{label} slugged to something a selector would choke on"
        );
    }
}

#[test]
fn a_file_name_survives_being_a_file_name() {
    use grow::util::file_name;
    assert_eq!(file_name("Person", "png"), "person.png");
    assert_eq!(file_name("walk cycle", "png"), "walk-cycle.png");
    assert_eq!(file_name("Tree/Trunk", "zip"), "tree-trunk.zip");
    assert_eq!(file_name("!!!", "png"), "untitled.png");
    // No extension means a folder inside an archive, and a directory whose
    // name ends in a dot is one some systems refuse.
    assert_eq!(file_name("Person", ""), "person");
}

// ---- the marquee ---------------------------------------------------------

/// A sheet with a run of pixels across the middle row, each a different value,
/// so what moved and what did not can be read off.
fn striped() -> Sheet {
    let mut sheet = Sheet::new("art-1", "Walk", 8, 3);
    for x in 0..8 {
        sheet.set(0, 0, x, 1, (x + 1) as u32);
    }
    sheet
}

fn middle_row(sheet: &Sheet) -> Vec<u32> {
    (0..8).map(|x| sheet.get(0, 0, x, 1)).collect()
}

#[test]
fn a_nudged_selection_moves_only_what_is_inside_it() {
    let mut sheet = striped();
    assert_eq!(middle_row(&sheet), vec![1, 2, 3, 4, 5, 6, 7, 8]);

    // The middle four, one to the right. The two either side stay put, the
    // rightmost of the four falls off the edge of the selection, and the space
    // it came from is cleared.
    sheet.shift_region(0, 0, (2, 1, 5, 1), 1, 0);
    assert_eq!(middle_row(&sheet), vec![1, 2, 0, 3, 4, 5, 7, 8]);
}

#[test]
fn a_selection_does_not_smear_past_its_own_edge() {
    let mut sheet = striped();
    sheet.shift_region(0, 0, (0, 1, 3, 1), 2, 0);
    // Only what fits inside the selection stays; 3 and 4 would have landed on
    // 5 and 6, which are not part of it.
    assert_eq!(middle_row(&sheet), vec![0, 0, 1, 2, 5, 6, 7, 8]);
}

#[test]
fn a_selection_moved_by_nothing_is_left_alone() {
    let mut sheet = striped();
    sheet.shift_region(0, 0, (1, 1, 4, 1), 0, 0);
    assert_eq!(middle_row(&sheet), vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn a_small_move_does_not_eat_what_it_has_not_read_yet() {
    // Moving in place, left to right, would copy 1 onto 2 and then copy the
    // copy onto 3, and so on. The whole selection is read out first.
    let mut sheet = striped();
    sheet.shift_region(0, 0, (0, 1, 7, 1), 1, 0);
    assert_eq!(middle_row(&sheet), vec![0, 1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn clearing_a_selection_empties_only_that_rectangle() {
    let mut sheet = striped();
    sheet.clear_region(0, 0, (2, 0, 4, 2));
    assert_eq!(middle_row(&sheet), vec![1, 2, 0, 0, 0, 6, 7, 8]);
}

#[test]
fn a_selection_off_the_edge_is_clamped_rather_than_refused() {
    let mut sheet = striped();
    // Nothing should panic, and nothing outside the sheet should be touched.
    sheet.clear_region(0, 0, (-4, -4, 2, 40));
    assert_eq!(middle_row(&sheet), vec![0, 0, 0, 4, 5, 6, 7, 8]);
    sheet.shift_region(0, 0, (5, 1, 40, 1), 1, 0);
    assert_eq!(middle_row(&sheet), vec![0, 0, 0, 4, 5, 0, 6, 7]);
}
