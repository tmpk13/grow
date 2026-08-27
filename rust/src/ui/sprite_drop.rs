//! Dropping images onto a settler animation.
//!
//! One card per motion, each its own drop target. A single image is read as a
//! strip of frames, several images are read as one frame each in the order
//! their names sort, and the frame count stays editable afterwards because the
//! sheet is kept whole rather than cut up.
//!
//! What a dropped file becomes is `ui/decode`'s business; this is only where a
//! motion's slot is, and how the clip in it is tuned afterwards.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::{Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, DragEvent, Element, Event, FileList, HtmlCanvasElement, ImageData,
};

use crate::app::{App, Handle};
use crate::civ::sprites::{guess_frames, Clip, Frame, Motion, MAX_FRAMES, MOTIONS};
use crate::ui::{
    app_bool, button, danger_button, document, el, input_el, note, number_field, on, section,
    select_field, NumOpts, Scope, Tap,
};
use crate::util::unpack_rgba;

/// Sheet size past which a project stops fitting comfortably in local storage.
/// Nothing is refused at it; the panel just says so, because a save that fails
/// is a worse way to find out.
const SIZE_WARN: usize = 1 << 20;

/// The whole "Settler sprites" section: the switch, a card per motion, and what
/// the sheets are costing.
pub fn sprites_section(app: &App, h: &Handle) -> Element {
    let sprites = &app.state.civ.sprites;
    let mut rows = vec![
        note(
            "Drop images on a motion to draw settlers with them instead of with the \
             generated body. One image is read as a strip of frames; several are read \
             as one frame each, in the order their names sort. A motion with nothing \
             on it borrows from a related one, so a single walk sheet is enough to \
             replace the settler everywhere.",
        ),
        app_bool(
            h,
            "Draw settlers from dropped images",
            sprites.enabled,
            Some("off keeps every sheet and goes back to the generated settler"),
            |app, v| {
                app.state.civ.sprites.enabled = v;
                app.sprites_changed();
            },
        ),
    ];
    for motion in MOTIONS {
        rows.push(slot_card(app, h, motion));
    }
    let bytes = sprites.bytes();
    if bytes > 0 {
        let text = format!("Sheets in this project: {}.", size_text(bytes));
        rows.push(note(&if bytes > SIZE_WARN {
            format!("{text} Past about a megabyte a project may be too large to save in the browser; drop smaller art or fewer frames.")
        } else {
            text
        }));
    }
    section("Settler sprites", rows)
}

fn size_text(bytes: usize) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1 << 20) as f64)
    } else {
        format!("{} kB", (bytes as f64 / 1024.0).round() as i64)
    }
}

fn meta_text(clip: Option<&Clip>) -> String {
    match clip.filter(|c| c.ready()) {
        Some(c) => format!("{} frames, {}x{}", c.frame_count(), c.frame_w(), c.h),
        None => "nothing dropped".to_string(),
    }
}

fn slot_card(app: &App, h: &Handle, motion: Motion) -> Element {
    let clip = app.state.civ.sprites.clip(motion);
    let meta = el("span").class("sprite-meta").text(&meta_text(clip)).get();
    let head = el("header")
        .class("sprite-head")
        .child(&el("span").class("sprite-name").text(motion.label()).get())
        .child(&meta)
        .get();
    let mut body = vec![
        head,
        el("p").class("field-hint").text(motion.hint()).get(),
        drop_zone(h, motion, clip),
        sheet_row(app, h, motion),
    ];
    if let Some(c) = clip.filter(|c| c.ready()) {
        let per_unit = if c.stride { "frames per cell walked" } else { "frames per second" };
        body.push(clip_num(
            h,
            motion,
            &meta,
            "Frames",
            c.frame_count() as f64,
            1.0,
            MAX_FRAMES as f64,
            1.0,
            Some("how many equal columns the sheet is read as"),
            |clip, v| clip.frames = (v.round() as i32).clamp(1, MAX_FRAMES),
        ));
        body.push(clip_num(
            h,
            motion,
            &meta,
            "Rate",
            c.fps,
            0.0,
            24.0,
            0.5,
            Some(per_unit),
            |clip, v| clip.fps = v,
        ));
        body.push(clip_bool(
            h,
            motion,
            "Tie to steps",
            c.stride,
            Some("the frame follows ground covered, so a walk never slides"),
            |clip, v| clip.stride = v,
        ));
        body.push(clip_num(
            h,
            motion,
            &meta,
            "Height (cells)",
            c.height,
            0.2,
            6.0,
            0.05,
            Some("width follows the shape of the frame"),
            |clip, v| clip.height = v,
        ));
        body.push(clip_num(
            h,
            motion,
            &meta,
            "Lift (cells)",
            c.lift,
            -1.0,
            2.0,
            0.05,
            Some("raises the art off the ground, for frames drawn with their own footing"),
            |clip, v| clip.lift = v,
        ));
        body.push(clip_bool(
            h,
            motion,
            "Mirror when facing left",
            c.flip,
            Some("off for art drawn facing the viewer"),
            |clip, v| clip.flip = v,
        ));
        body.push(clip_bool(
            h,
            motion,
            "Mirror the art",
            c.mirror,
            Some("for a sheet drawn facing the other way than the settler walks"),
            |clip, v| clip.mirror = v,
        ));
        if let Some(sheet) = app.state.art.find(&c.sheet) {
            let h2 = h.clone();
            let id = sheet.id.clone();
            body.push(
                el("div")
                    .class("btn-row")
                    .child(&el("span").class("field-hint").text("drawn in the editor").get())
                    .child(&button(&format!("Take {} again", sheet.name), Scope::Panel, move || {
                        let mut sh = h2.borrow_mut();
                        let id = id.clone();
                        build_from_sheet(&mut sh.app, motion, &id);
                    }))
                    .get(),
            );
        }
        let h2 = h.clone();
        body.push(
            el("div")
                .class("btn-row")
                .child(&danger_button("Clear", Scope::Panel, move || {
                    let mut sh = h2.borrow_mut();
                    sh.app.state.civ.sprites.set(motion, None);
                    sh.app.sprites_changed();
                    sh.app.rebuild_panel = true;
                }))
                .get(),
        );
    }
    el("div").class("sprite-slot").children(body).get()
}

/// Pointing a motion at a sheet drawn in the sprite editor, which is the other
/// way art gets here. The sheet is copied into a clip rather than followed, so
/// carrying on drawing does not change the settlers until it is sent again.
fn sheet_row(app: &App, h: &Handle, motion: Motion) -> Element {
    let options = app.state.art.options();
    if options.is_empty() {
        return el("div").get();
    }
    let first = options.first().map(|(id, _)| id.clone()).unwrap_or_default();
    let chosen = Rc::new(RefCell::new(first.clone()));
    let picker = {
        let chosen = chosen.clone();
        select_field("From editor", &first, &options, None, move |v| {
            *chosen.borrow_mut() = v;
        })
    };
    let send = {
        let h2 = h.clone();
        let chosen = chosen.clone();
        button("Use sheet", Scope::Panel, move || {
            let id = chosen.borrow().clone();
            let mut sh = h2.borrow_mut();
            build_from_sheet(&mut sh.app, motion, &id);
        })
    };
    el("div").class("sprite-from").child(&picker).child(&send).get()
}

/// Builds a motion's clip from a sheet, whether it is being pointed at one for
/// the first time or being sent the same sheet again.
pub fn build_from_sheet(app: &mut App, motion: Motion, id: &str) {
    match app.state.art.find(id).and_then(Clip::from_sheet) {
        Some(clip) => {
            app.state.civ.sprites.enabled = true;
            apply_clip(app, motion, clip);
        }
        None => app.set_note("nothing drawn on that sheet"),
    }
    app.rebuild_panel = true;
}

// ---- the drop target -----------------------------------------------------

fn drop_zone(h: &Handle, motion: Motion, clip: Option<&Clip>) -> Element {
    let picker = input_el("file").tap(|i| {
        i.set_accept("image/*");
        i.set_multiple(true);
        let _ = i.set_attribute("hidden", "hidden");
    });

    let zone = el("div").class("dropzone").get();
    if let Some(c) = clip.filter(|c| c.ready()) {
        if let Some(strip) = strip_canvas(c) {
            let _ = zone.append_child(&strip);
        }
    }
    let hint = if clip.is_some_and(|c| c.ready()) {
        "Drop images to replace"
    } else {
        "Drop a strip, or one image per frame"
    };
    let _ = zone.append_child(&el("span").class("dropzone-hint").text(hint).get());
    let _ = zone.append_child(picker.unchecked_ref());

    for event in ["dragenter", "dragover"] {
        let zone2 = zone.clone();
        on(zone.unchecked_ref(), event, Scope::Panel, move |e: Event| {
            e.prevent_default();
            let _ = zone2.class_list().add_1("over");
        });
    }
    {
        let zone2 = zone.clone();
        on(zone.unchecked_ref(), "dragleave", Scope::Panel, move |_| {
            let _ = zone2.class_list().remove_1("over");
        });
    }
    {
        let zone2 = zone.clone();
        let h2 = h.clone();
        on(zone.unchecked_ref(), "drop", Scope::Panel, move |e: Event| {
            e.prevent_default();
            let _ = zone2.class_list().remove_1("over");
            let files = e
                .dyn_ref::<DragEvent>()
                .and_then(|d| d.data_transfer())
                .and_then(|t| t.files());
            if let Some(files) = files {
                load_files(&h2, motion, files);
            }
        });
    }
    {
        // The same thing without a mouse that can drag: the hidden picker is
        // what a click and a keyboard both reach.
        let picker2 = picker.clone();
        on(zone.unchecked_ref(), "click", Scope::Panel, move |_| {
            picker2.click();
        });
    }
    {
        let picker2 = picker.clone();
        on(zone.unchecked_ref(), "keydown", Scope::Panel, move |e: Event| {
            let key = match e.dyn_ref::<web_sys::KeyboardEvent>() {
                Some(k) => k.key(),
                None => return,
            };
            if key == "Enter" || key == " " {
                e.prevent_default();
                picker2.click();
            }
        });
    }
    {
        // The picker sits inside the zone, so the click that opens it would
        // bubble straight back to the handler that opened it.
        on(picker.unchecked_ref(), "click", Scope::Panel, move |e: Event| {
            e.stop_propagation();
        });
    }
    {
        let picker2 = picker.clone();
        let h2 = h.clone();
        on(picker.unchecked_ref(), "change", Scope::Panel, move |_| {
            if let Some(files) = picker2.files() {
                load_files(&h2, motion, files);
            }
            picker2.set_value("");
        });
    }
    let _ = zone.set_attribute("role", "button");
    let _ = zone.set_attribute("tabindex", "0");
    zone
}

/// The sheet as it will be read: every frame, side by side, at one pixel each.
/// The canvas is sized in source pixels and scaled by the stylesheet, so the
/// preview is the art rather than a resampling of it.
fn strip_canvas(clip: &Clip) -> Option<Element> {
    let fw = clip.frame_w();
    let frames = clip.frame_count();
    let (w, h) = (fw * frames, clip.h);
    if w <= 0 || h <= 0 {
        return None;
    }
    let canvas = document()
        .create_element("canvas")
        .ok()?
        .dyn_into::<HtmlCanvasElement>()
        .ok()?;
    canvas.set_class_name("sprite-strip");
    canvas.set_width(w as u32);
    canvas.set_height(h as u32);
    let ctx = canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()?;
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = unpack_rgba(clip.pixel(x / fw, x % fw, y));
            bytes.extend_from_slice(&[c.r, c.g, c.b, c.a]);
        }
    }
    let image =
        ImageData::new_with_u8_clamped_array_and_sh(Clamped(&bytes), w as u32, h as u32).ok()?;
    let _ = ctx.put_image_data(&image, 0.0, 0.0);
    Some(canvas.unchecked_into())
}

// ---- reading what was dropped --------------------------------------------

fn load_files(h: &Handle, motion: Motion, files: FileList) {
    let h = h.clone();
    crate::ui::decode::read_files(files, move |frames, strip, source| {
        apply(&h, motion, frames, strip, &source);
    });
}

fn apply(h: &Handle, motion: Motion, frames: Vec<Frame>, strip: bool, source: &str) {
    let mut sh = h.borrow_mut();
    let built = if strip {
        frames.into_iter().next().and_then(|(w, height, px)| {
            Clip::from_strip(w, height, px, guess_frames(w, height), source.to_string())
        })
    } else {
        Clip::from_frames(frames, source.to_string())
    };
    match built {
        Some(clip) => apply_clip(&mut sh.app, motion, clip),
        None => sh.app.set_note("nothing readable in that drop"),
    }
    sh.app.rebuild_panel = true;
}

/// Drops a freshly built clip into a motion. Playback that has already been
/// tuned for this motion outlives the art it was tuned on; only a fresh slot
/// takes the defaults. Which sheet a clip came from is part of the art rather
/// than part of the tuning, so it is not carried over.
pub fn apply_clip(app: &mut App, motion: Motion, mut clip: Clip) {
    app.record("settler art", false);
    match app.state.civ.sprites.clip(motion) {
        Some(old) => {
            clip.fps = old.fps;
            clip.stride = old.stride;
            clip.height = old.height;
            clip.lift = old.lift;
            clip.flip = old.flip;
            clip.mirror = old.mirror;
        }
        None => {
            let (fps, stride) = motion.playback();
            if clip.fps <= 0.0 {
                clip.fps = fps;
            }
            clip.stride = stride;
        }
    }
    let count = clip.frame_count();
    app.state.civ.sprites.set(motion, Some(clip));
    app.set_note(&format!("{}: {count} frames", motion.label().to_lowercase()));
    app.sprites_changed();
}

// ---- fields --------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn clip_num(
    h: &Handle,
    motion: Motion,
    meta: &Element,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: impl Fn(&mut Clip, f64) + 'static,
) -> Element {
    let h2 = h.clone();
    let meta = meta.clone();
    let key = label.to_string();
    number_field(label, value, NumOpts { min, max, step }, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, true);
        if let Some(clip) = sh.app.state.civ.sprites.slot_mut(motion).as_mut() {
            apply(clip, v);
        }
        // The header carries the frame size, which the frame count changes, and
        // rebuilding the whole panel on every drag of a slider would not.
        meta.set_text_content(Some(&meta_text(sh.app.state.civ.sprites.clip(motion))));
        sh.app.sprites_changed();
    })
}

fn clip_bool(
    h: &Handle,
    motion: Motion,
    label: &str,
    value: bool,
    hint: Option<&str>,
    apply: impl Fn(&mut Clip, bool) + 'static,
) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    crate::ui::bool_field(label, value, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, false);
        if let Some(clip) = sh.app.state.civ.sprites.slot_mut(motion).as_mut() {
            apply(clip, v);
        }
        sh.app.sprites_changed();
    })
}
