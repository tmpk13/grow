//! The drawing side of the tool: sheets in the sprite editor, the sizing a
//! dropped sprite comes out at, and how a sampling box is read as a ramp.

use grow::art::{ArtLibrary, Sheet};
use grow::civ::sprites::Clip;
use grow::sampler::{ramp_pick, Materials};
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
