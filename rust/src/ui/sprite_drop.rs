//! Dropping images onto a settler animation.
//!
//! One card per motion, each its own drop target. A single image is read as a
//! strip of frames, several images are read as one frame each in the order
//! their names sort, and the frame count stays editable afterwards because the
//! sheet is kept whole rather than cut up.
//!
//! Decoding goes through the browser: a file becomes an object URL, an image
//! element, a canvas and finally packed pixels, which is why none of this lives
//! next to the clip itself.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, DragEvent, Element, Event, File, FileList, HtmlCanvasElement,
    HtmlImageElement, ImageData,
};

use crate::app::{App, Handle};
use crate::civ::sprites::{
    guess_frames, natural_cmp, Clip, Frame, Motion, ALPHA_CUT, MAX_FRAMES, MOTIONS,
};
use crate::ui::{
    app_bool, danger_button, document, el, input_el, note, number_field, on, section, NumOpts,
    Scope, Tap,
};
use crate::util::{pack_rgba, unpack_rgba};

/// Whoever gets there first: an image either loads or it fails, and the caller
/// is told once either way.
type Sink = Rc<RefCell<Option<Box<dyn FnOnce(Option<Frame>)>>>>;

/// Source images larger than this are drawn down on the way in. Nothing on the
/// map is read at anything near it, and walking a photograph pixel by pixel is
/// how a drop turns into a stall.
const MAX_SOURCE_PX: i32 = 1024;

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
    let mut list: Vec<File> = (0..files.length()).filter_map(|i| files.get(i)).collect();
    list.sort_by(|a, b| natural_cmp(&a.name(), &b.name()));
    list.truncate(MAX_FRAMES as usize);
    if list.is_empty() {
        return;
    }
    let strip = list.len() == 1;
    let source = if strip {
        list[0].name()
    } else {
        format!("{} images", list.len())
    };
    // Every file decodes on its own callback, so the slots are filled out of
    // order and only the last one home does anything with them.
    let slots: Rc<RefCell<Vec<Option<Frame>>>> = Rc::new(RefCell::new(vec![None; list.len()]));
    let left = Rc::new(Cell::new(list.len()));
    for (i, file) in list.into_iter().enumerate() {
        let slots = slots.clone();
        let left = left.clone();
        let h = h.clone();
        let source = source.clone();
        decode(&file, move |frame| {
            slots.borrow_mut()[i] = frame;
            left.set(left.get().saturating_sub(1));
            if left.get() > 0 {
                return;
            }
            let frames: Vec<Frame> = std::mem::take(&mut *slots.borrow_mut())
                .into_iter()
                .flatten()
                .collect();
            apply(&h, motion, frames, strip, &source);
        });
    }
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
        Some(mut clip) => {
            // Playback that has already been tuned for this motion outlives the
            // art it was tuned on; only a fresh slot takes the defaults.
            match sh.app.state.civ.sprites.clip(motion) {
                Some(old) => {
                    clip.fps = old.fps;
                    clip.stride = old.stride;
                    clip.height = old.height;
                    clip.lift = old.lift;
                    clip.flip = old.flip;
                }
                None => {
                    let (fps, stride) = motion.playback();
                    clip.fps = fps;
                    clip.stride = stride;
                }
            }
            let count = clip.frame_count();
            sh.app.state.civ.sprites.set(motion, Some(clip));
            sh.app
                .set_note(&format!("{}: {count} frames", motion.label().to_lowercase()));
        }
        None => sh.app.set_note("nothing readable in that drop"),
    }
    sh.app.sprites_changed();
    sh.app.rebuild_panel = true;
}

/// One file to one frame of packed pixels, through an image element and a
/// canvas. Anything that fails on the way, a file that is not an image
/// included, arrives as nothing rather than as an error nobody asked for.
fn decode(file: &File, done: impl FnOnce(Option<Frame>) + 'static) {
    let sink: Sink = Rc::new(RefCell::new(Some(Box::new(done))));
    let fire = |sink: &Sink, out: Option<Frame>| {
        if let Some(f) = sink.borrow_mut().take() {
            f(out);
        }
    };
    let url = match web_sys::Url::create_object_url_with_blob(file.as_ref()) {
        Ok(url) => url,
        Err(_) => return fire(&sink, None),
    };
    let img = match HtmlImageElement::new() {
        Ok(img) => img,
        Err(_) => {
            let _ = web_sys::Url::revoke_object_url(&url);
            return fire(&sink, None);
        }
    };
    {
        let sink = sink.clone();
        let url = url.clone();
        let onload = Closure::once_into_js(move |e: Event| {
            let out = e
                .target()
                .and_then(|t| t.dyn_into::<HtmlImageElement>().ok())
                .and_then(|img| image_pixels(&img));
            let _ = web_sys::Url::revoke_object_url(&url);
            fire(&sink, out);
        });
        img.set_onload(Some(onload.unchecked_ref()));
    }
    {
        let sink = sink.clone();
        let url = url.clone();
        let onerror = Closure::once_into_js(move |_: JsValue| {
            let _ = web_sys::Url::revoke_object_url(&url);
            fire(&sink, None);
        });
        img.set_onerror(Some(onerror.unchecked_ref()));
    }
    img.set_src(&url);
}

/// A loaded image as packed pixels. Anything past the source cap is drawn down
/// on the way in, without smoothing, so pixel art keeps its edges.
fn image_pixels(img: &HtmlImageElement) -> Option<Frame> {
    let (sw, sh) = (img.natural_width() as i32, img.natural_height() as i32);
    if sw <= 0 || sh <= 0 {
        return None;
    }
    let ratio = (sw as f64 / MAX_SOURCE_PX as f64)
        .max(sh as f64 / MAX_SOURCE_PX as f64)
        .max(1.0);
    let w = ((sw as f64 / ratio).round() as i32).max(1);
    let h = ((sh as f64 / ratio).round() as i32).max(1);
    let canvas = document()
        .create_element("canvas")
        .ok()?
        .dyn_into::<HtmlCanvasElement>()
        .ok()?;
    canvas.set_width(w as u32);
    canvas.set_height(h as u32);
    let ctx = canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()?;
    ctx.set_image_smoothing_enabled(false);
    ctx.draw_image_with_html_image_element_and_dw_and_dh(img, 0.0, 0.0, w as f64, h as f64)
        .ok()?;
    let data = ctx.get_image_data(0.0, 0.0, w as f64, h as f64).ok()?;
    let bytes = data.data();
    let mut px = Vec::with_capacity((w * h) as usize);
    let mut at = 0;
    while at + 3 < bytes.len() {
        px.push(if bytes[at + 3] < ALPHA_CUT {
            0
        } else {
            pack_rgba(bytes[at] as i32, bytes[at + 1] as i32, bytes[at + 2] as i32, 255)
        });
        at += 4;
    }
    px.resize((w * h) as usize, 0);
    Some((w, h, px))
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
    number_field(label, value, NumOpts { min, max, step }, hint, move |v| {
        let mut sh = h2.borrow_mut();
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
    crate::ui::bool_field(label, value, hint, move |v| {
        let mut sh = h2.borrow_mut();
        if let Some(clip) = sh.app.state.civ.sprites.slot_mut(motion).as_mut() {
            apply(clip, v);
        }
        sh.app.sprites_changed();
    })
}
