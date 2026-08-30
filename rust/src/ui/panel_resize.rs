//! The handle on the side menu's edge. Dragging it sets how much of the
//! window the menu gets, in rem so the width rides along when the text is
//! rescaled; the stage takes whatever is left, and the canvas follows through
//! the same resize observer that watches the window. A double press puts the
//! stylesheet's own width back.
//!
//! The width is written to a custom property rather than to the element, so
//! the stylesheet keeps the last word on its bounds, and the preference is
//! saved once at the end of the drag rather than at every pointer step.

use std::cell::Cell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use web_sys::PointerEvent;

use crate::ui::prefs::{Prefs, PANEL_REM_MAX, PANEL_REM_MIN};
use crate::ui::{by_id, document, on, window, Scope};

/// One rem in device pixels right now. The text scale multiplies the root
/// font size, so this is read at every step rather than once.
fn rem_px() -> f64 {
    document()
        .document_element()
        .and_then(|e| window().get_computed_style(&e).ok().flatten())
        .and_then(|s| s.get_property_value("font-size").ok())
        .and_then(|v| v.trim_end_matches("px").parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(16.0)
}

fn set_width(rem: f64) {
    if let Some(root) = document()
        .document_element()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = root.style().set_property("--panel-w", &format!("{rem}rem"));
    }
}

pub fn bind() {
    let handle = match by_id("panel-resize") {
        Some(n) => n,
        None => return,
    };

    // Written by the move steps, read once by the release; zero between drags.
    let dragged_to = Rc::new(Cell::new(0.0f64));

    {
        let handle2 = handle.clone();
        on(handle.unchecked_ref(), "pointerdown", Scope::Global, move |e| {
            let e: PointerEvent = match e.dyn_into() {
                Ok(e) => e,
                Err(_) => return,
            };
            e.prevent_default();
            let _ = handle2.set_pointer_capture(e.pointer_id());
            let _ = handle2.class_list().add_1("dragging");
            // The layout eases width changes, which would trail a drag; the
            // class holds the easing off while the pointer leads.
            if let Some(body) = document().body() {
                let _ = body.class_list().add_1("resizing");
            }
        });
    }

    {
        let handle2 = handle.clone();
        let dragged = dragged_to.clone();
        on(handle.unchecked_ref(), "pointermove", Scope::Global, move |e| {
            if !handle2.class_list().contains("dragging") {
                return;
            }
            let e: PointerEvent = match e.dyn_into() {
                Ok(e) => e,
                Err(_) => return,
            };
            let left = document()
                .query_selector(".panel")
                .ok()
                .flatten()
                .map(|p| p.get_bounding_client_rect().left())
                .unwrap_or(0.0);
            let rem = ((e.client_x() as f64 - left) / rem_px()).clamp(PANEL_REM_MIN, PANEL_REM_MAX);
            dragged.set(rem);
            set_width(rem);
        });
    }

    for done in ["pointerup", "pointercancel"] {
        let handle2 = handle.clone();
        let dragged = dragged_to.clone();
        on(handle.unchecked_ref(), done, Scope::Global, move |_| {
            if !handle2.class_list().contains("dragging") {
                return;
            }
            let _ = handle2.class_list().remove_1("dragging");
            if let Some(body) = document().body() {
                let _ = body.class_list().remove_1("resizing");
            }
            let rem = dragged.replace(0.0);
            if rem > 0.0 {
                let mut prefs = Prefs::load();
                prefs.panel_rem = rem;
                prefs.save();
            }
        });
    }

    on(handle.unchecked_ref(), "dblclick", Scope::Global, move |_| {
        let mut prefs = Prefs::load();
        prefs.panel_rem = 0.0;
        prefs.save();
        prefs.apply();
    });
}
