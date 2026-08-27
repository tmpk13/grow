//! Reading dropped image files into packed pixels.
//!
//! Decoding goes through the browser: a file becomes an object URL, an image
//! element, a canvas and finally packed pixels, which is why none of this lives
//! next to the buffers it ends up in. Both the settler motion slots and the
//! sprite editor read images the same way, so both read them from here.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Event, File, FileList, HtmlCanvasElement, HtmlImageElement};

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

