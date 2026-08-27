//! The drawing side of the tool: sheets in the sprite editor, the sizing a
//! dropped sprite comes out at, and how a sampling box is read as a ramp.

use grow::art::{ArtLibrary, Sheet};
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
fn a_sheet_of_nothing_is_not_offered_as_settler_art() {
    let blank = Sheet::new("blank", "Blank", 4, 4);
    assert!(!blank.any());
    assert!(Clip::from_sheet(&blank).is_none());

    let sheet = sheet_with_two_layers();
    let clip = Clip::from_sheet(&sheet).expect("a drawn sheet makes a clip");
    assert_eq!(clip.frame_count(), 2);
    // Only the middle row is drawn, so the clip is cropped to it and reads the
    // flattened stack rather than any one layer.
    assert_eq!(clip.h, 1);
    assert_eq!(clip.frame_w(), 4);
    assert_eq!(clip.pixel(0, 1, 0), BLUE);
    assert_eq!(clip.pixel(1, 1, 0), RED);
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
fn how_much_a_sprite_was_padded_does_not_change_how_large_it_comes_out() {
    // The same art on two very different canvases. What is drawn is four by
    // six either way, so the clip has to be four by six either way; anything
    // else means the drawn height is being measured against the padding.
    let tight = padded_strip(2, 6, 8, 4, 6, 1);
    let loose = padded_strip(2, 32, 40, 4, 6, 9);
    for clip in [&tight, &loose] {
        assert_eq!(clip.frame_w(), 4, "the art was not what the width was read from");
        assert_eq!(clip.h, 6, "the art was not what the height was read from");
        assert_eq!(clip.frame_count(), 2);
        assert_eq!(clip.pixel(0, 0, 0), RED, "the crop cut into the art");
        assert_eq!(clip.pixel(1, 3, 5), RED);
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
