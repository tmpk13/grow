//! The view menu: what the stage draws over the map, as opposed to what the
//! map is. It lives in the side panel rather than in the top bar because it
//! grew past what a row of checkboxes can hold, and because none of it is a
//! setting of the project.
//!
//! It is rebuilt whole whenever one of its own switches changes what the others
//! should read, which is why it has a listener scope to itself.

use web_sys::Element;

use crate::app::{App, Handle, Mode};
use crate::civ::config::LABEL_KINDS;
use wasm_bindgen::JsCast;

use crate::ui::{append, by_id, clear, clear_scope, el, on, toggle_button, Scope};

/// Builds the block from scratch. Safe to call from inside one of its own
/// listeners: the old listeners go with the old nodes.
pub fn build(app: &mut App, h: &Handle) {
    let root = match by_id("view-body") {
        Some(n) => n,
        None => return,
    };
    clear(&root);
    clear_scope(Scope::View);

    let mut rows: Vec<Element> = vec![
        switch(h, "Grid", app.viewport.show_grid, |app, v| app.viewport.show_grid = v),
        switch(h, "Occupancy", app.viewport.show_occupancy, |app, v| {
            app.viewport.show_occupancy = v
        }),
    ];

    if app.mode == Mode::Settlement {
        let view = &app.state.civ.view;
        rows.push(el("hr").class("view-rule").get());
        rows.push(switch(h, "Labels", view.labels, |app, v| {
            app.state.civ.view.labels = v;
            app.request_save();
        }));

        // The kinds are shown whatever the master switch says, so turning names
        // off and on again does not look like it cleared them.
        let all = view.all_labels();
        let kinds = el("div").class("view-kinds").get();
        {
            let h2 = h.clone();
            append(
                &kinds,
                toggle_button("All", all, Scope::View, move |on| {
                    let mut sh = h2.borrow_mut();
                    let sh = &mut *sh;
                    for (kind, _) in LABEL_KINDS {
                        sh.app.state.civ.view.set_label(kind, on);
                    }
                    sh.app.request_save();
                    build(&mut sh.app, &h2);
                }),
            );
        }
        for (kind, label) in LABEL_KINDS {
            let h2 = h.clone();
            let on_now = app.state.civ.view.label_flag(kind);
            append(
                &kinds,
                toggle_button(label, on_now, Scope::View, move |on| {
                    let mut sh = h2.borrow_mut();
                    let sh = &mut *sh;
                    sh.app.state.civ.view.set_label(kind, on);
                    sh.app.request_save();
                    build(&mut sh.app, &h2);
                }),
            );
        }
        rows.push(kinds);
    }

    for row in rows {
        append(&root, row);
    }
}

/// One switch that only ever writes to the app: no rebuild, because nothing
/// else on the menu reads it.
fn switch(
    h: &Handle,
    label: &str,
    on_now: bool,
    apply: fn(&mut App, bool),
) -> Element {
    let h2 = h.clone();
    el("div")
        .class("view-row")
        .child(&toggle_button(label, on_now, Scope::View, move |v| {
            apply(&mut h2.borrow_mut().app, v);
        }))
        .get()
}

/// Puts the fold state back the way it was left. The menu is rebuilt on every
/// mode change and would otherwise spring open each time.
pub fn restore_fold(open: bool) {
    if let Some(node) = by_id("view-menu") {
        let _ = if open {
            node.set_attribute("open", "open")
        } else {
            node.remove_attribute("open")
        };
    }
}

/// Remembers the fold with the rest of the window preferences.
pub fn bind_fold() {
    let node = match by_id("view-menu") {
        Some(n) => n,
        None => return,
    };
    on(node.unchecked_ref(), "toggle", Scope::Global, |e| {
        let open = e
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .map(|n| n.has_attribute("open"))
            .unwrap_or(true);
        let mut prefs = crate::ui::prefs::Prefs::load();
        prefs.view_open = open;
        prefs.save();
    });
}
