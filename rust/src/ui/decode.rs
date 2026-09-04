//! Reading dropped image files into packed pixels.
//!
//! Decoding goes through the browser: a file becomes an object URL, an image
//! element, a canvas and finally packed pixels, which is why none of this lives
//! next to the buffers it ends up in. Both the person motion slots and the
//! sprite editor read images the same way, so both read them from here.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    CanvasRenderingContext2d, Element, Event, File, FileList, HtmlCanvasElement, HtmlImageElement,
};

use crate::app::{App, Handle};
use crate::civ::sprites::{natural_cmp, Frame, ALPHA_CUT, MAX_FRAMES};
use crate::ui::document;
use crate::util::pack_rgba;

/// Whoever gets there first: an image either loads or it fails, and the caller
/// is told once either way.
type Sink = Rc<RefCell<Option<Box<dyn FnOnce(Option<Frame>)>>>>;

/// The caller, held until the last file is home. Every image decodes on its own
/// callback, so whichever one finishes last is the one that reports.
type Done = Rc<RefCell<Option<Box<dyn FnOnce(Vec<Frame>, bool, String)>>>>;

/// Source images larger than this are drawn down on the way in. Nothing the
/// tool draws is read at anything near it, and walking a photograph pixel by
/// pixel is how a drop turns into a stall.
const MAX_SOURCE_PX: i32 = 1024;

/// Every file in a drop, decoded and handed over together in the order their
/// names sort. Files that are not images arrive as nothing and are dropped, so
/// the caller sees only what could be read.
pub fn read_files(files: FileList, done: impl FnOnce(Vec<Frame>, bool, String) + 'static) {
    let mut list: Vec<File> = (0..files.length()).filter_map(|i| files.get(i)).collect();
    list.sort_by(|a, b| natural_cmp(&a.name(), &b.name()));
    list.truncate(MAX_FRAMES as usize);
    if list.is_empty() {
        return;
    }
    let single = list.len() == 1;
    let source = if single {
        list[0].name()
    } else {
        format!("{} images", list.len())
    };
    // The slots are filled out of order, so they are collected rather than
    // appended to.
    let slots: Rc<RefCell<Vec<Option<Frame>>>> = Rc::new(RefCell::new(vec![None; list.len()]));
    let left = Rc::new(Cell::new(list.len()));
    let done: Done = Rc::new(RefCell::new(Some(Box::new(done))));
    for (i, file) in list.into_iter().enumerate() {
        let slots = slots.clone();
        let left = left.clone();
        let done = done.clone();
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
            if let Some(f) = done.borrow_mut().take() {
                f(frames, single, source);
            }
        });
    }
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


// ---- how large a dropped picture was drawn -------------------------------

/// What every drop is read at until it is told otherwise, from this browser's
/// own settings. Zero means work it out from the picture.
pub fn default_scale() -> i32 {
    crate::ui::prefs::Prefs::load().import_px.max(0)
}

/// What one drop target is set to. Each of them keeps its own answer, because
/// people are drawn at one size and the things they build at another, and
/// neither of them is the size a reference photograph was scanned at.
pub fn scale_of(app: &App, key: &str) -> i32 {
    app.ui
        .import_px
        .iter()
        .find(|(k, _)| k == key)
        .map(|&(_, n)| n)
        .unwrap_or_else(default_scale)
}

pub fn set_scale(app: &mut App, key: &str, n: i32) {
    let n = n.max(0);
    match app.ui.import_px.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = n,
        None => app.ui.import_px.push((key.to_string(), n)),
    }
}

/// What was dropped, at one pixel per block it was drawn in. The scale used
/// comes back with it, since it may have been guessed and the note says so.
///
/// Guessed from the first picture and applied to all of them: a set of frames
/// dropped together was drawn together, and reading one of them at a different
/// scale from the rest would leave a walk cycle that changes size as it plays.
pub fn scaled(frames: Vec<Frame>, want: i32) -> (Vec<Frame>, i32) {
    let n = if want > 0 {
        want
    } else {
        match frames.first() {
            Some((w, h, px)) => crate::civ::sprites::pixel_size(*w, *h, px),
            None => 1,
        }
    };
    if n <= 1 {
        return (frames, 1);
    }
    (frames.into_iter().map(|f| crate::civ::sprites::shrink(f, n)).collect(), n)
}

/// The number box beside a drop target.
pub fn scale_field(app: &App, h: &Handle, key: &str, hint: &str) -> Element {
    let h2 = h.clone();
    let key2 = key.to_string();
    let value = scale_of(app, key) as f64;
    crate::ui::count_field(
        "Picture pixels to a pixel",
        value,
        0.0,
        1.0,
        Some(hint),
        move |v| {
            let mut sh = h2.borrow_mut();
            set_scale(&mut sh.app, &key2, v as i32);
            sh.app.rebuild_panel = true;
        },
    )
}

/// The one every drop target starts from, kept with this browser rather than
/// with the project.
pub fn default_scale_field(h: &Handle) -> Element {
    let h2 = h.clone();
    crate::ui::count_field(
        "Picture pixels to a pixel",
        default_scale() as f64,
        0.0,
        1.0,
        Some(
            "what every drop starts at: 0 works it out from the picture, 1 takes it as it \
             is, 8 reads art drawn eight screen pixels to a pixel",
        ),
        move |v| {
            let mut prefs = crate::ui::prefs::Prefs::load();
            prefs.import_px = (v as i32).max(0);
            prefs.save();
            let mut sh = h2.borrow_mut();
            sh.app.ui.import_px.clear();
            sh.app.rebuild_panel = true;
        },
    )
}
