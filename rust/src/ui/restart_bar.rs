//! The bar that says the running world is not the one the settings describe.
//!
//! A setting a world is built from does not rebuild it as it is dragged. It is
//! starred in the panel, listed here, and waits for Apply. Leaving the panel
//! with one waiting asks first, because the alternative is silently throwing
//! the change away or silently restarting on the way out.

use std::rc::Rc;

use wasm_bindgen::JsCast;

use crate::app::{App, Handle};
use crate::ui::{by_id, clear, clear_scope, el, on, slug, Scope};

/// Rewrites the line of text and the stars in the panel to match what is
/// waiting. The two buttons are in the page already, so nothing here creates a
/// node or a listener: this runs on every value a slider passes through.
pub fn sync(app: &App) {
    let which = app.restart_target();
    let waiting = app.waiting(which);
    star_fields(&waiting);

    let bar = match by_id("restart-bar") {
        Some(n) => n,
        None => return,
    };
    if waiting.is_empty() {
        let _ = bar.set_attribute("hidden", "hidden");
        return;
    }
    let _ = bar.remove_attribute("hidden");
    let _ = bar.set_attribute("title", &waiting.join(", "));
    if let Some(node) = by_id("restart-what") {
        let what = if waiting.len() == 1 {
            format!("{} is waiting on a rebuild", waiting[0])
        } else {
            format!("{} settings are waiting on a rebuild", waiting.len())
        };
        node.set_text_content(Some(&what));
    }
}

/// Wires the two buttons, once, for the life of the page.
pub fn mount(h: &Handle) {
    let bar = match by_id("restart-bar") {
        Some(n) => n,
        None => return,
    };
    let h2 = h.clone();
    on(bar.unchecked_ref(), "click", Scope::Global, move |e| {
        let action = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|t| t.closest("[data-do]").ok().flatten())
            .and_then(|n| n.get_attribute("data-do"));
        let mut sh = h2.borrow_mut();
        match action.as_deref() {
            Some("apply") => sh.app.apply_restarts(),
            Some("discard") => sh.app.discard_restarts(),
            _ => return,
        }
        sync(&sh.app);
        sh.app.rebuild_panel();
    });
}

/// Marks the fields in the open panel that the running world does not have.
fn star_fields(waiting: &[&str]) {
    let panel = match by_id("panel-body") {
        Some(n) => n,
        None => return,
    };
    let rows = match panel.query_selector_all("[data-restart]") {
        Ok(r) => r,
        Err(_) => return,
    };
    for i in 0..rows.length() {
        let node = match rows.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) {
            Some(n) => n,
            None => continue,
        };
        let anchor = node.get_attribute("data-find").unwrap_or_default();
        let on_now = waiting.iter().any(|w| slug(w) == anchor);
        let _ = if on_now {
            node.class_list().add_1("waiting")
        } else {
            node.class_list().remove_1("waiting")
        };
    }
}

/// Runs `go`, or asks first if the world would be left behind. The three ways
/// out are the three things somebody could mean: build it, drop it, or stay
/// and keep working on it.
pub fn leaving(h: &Handle, go: Rc<dyn Fn(&Handle)>) {
    let waiting = {
        let sh = h.borrow();
        sh.app.waiting(sh.app.restart_target()).len()
    };
    if waiting == 0 {
        go(h);
        return;
    }
    let dialog = match by_id("confirm") {
        Some(n) => n,
        None => {
            go(h);
            return;
        }
    };
    clear(&dialog);
    clear_scope(Scope::Dialog);

    let plural = if waiting == 1 { "change is" } else { "changes are" };
    let body = el("div")
        .class("confirm-body")
        .child(&el("h3").text("Leave without applying?").get())
        .child(
            &el("p")
                .class("note")
                .text(&format!(
                    "{waiting} {plural} waiting on a rebuild. The world you are looking at was \
                     not built from them."
                ))
                .get(),
        )
        .get();

    let row = el("div").class("btn-row").get();
    for (label, what) in [
        ("Apply and go", "apply"),
        ("Discard and go", "discard"),
        ("Stay here", "stay"),
    ] {
        let h2 = h.clone();
        let go = go.clone();
        let button = el("button")
            .class(if what == "stay" { "btn" } else { "btn accent" })
            .attr("type", "button")
            .attr("data-do", what)
            .text(label)
            .on("click", Scope::Dialog, move |_| {
                close();
                match what {
                    "apply" => {
                        let mut sh = h2.borrow_mut();
                        sh.app.apply_restarts();
                        drop(sh);
                        go(&h2);
                    }
                    "discard" => {
                        let mut sh = h2.borrow_mut();
                        sh.app.discard_restarts();
                        drop(sh);
                        go(&h2);
                    }
                    _ => {
                        let sh = h2.borrow();
                        sync(&sh.app);
                    }
                }
            })
            .get();
        let _ = row.append_child(&button);
    }
    let _ = body.append_child(&row);
    let _ = dialog.append_child(&body);
    let _ = dialog.remove_attribute("hidden");
}

fn close() {
    if let Some(dialog) = by_id("confirm") {
        let _ = dialog.set_attribute("hidden", "hidden");
    }
}
