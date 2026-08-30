//! The view menu: what the stage draws over the map, as opposed to what the
//! map is. It is a dropdown in the top bar rather than a block of the side
//! panel, because it is about the stage and the side panel folds away with
//! everything else; none of it is a setting of the project. The sprite editor
//! draws none of its overlays, so the whole menu is hidden there.
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

    // The sprite editor draws no grid, no occupancy and no labels, so the
    // menu is not merely empty there: it is gone, folded shut for the way
    // back. The body stays empty too, which keeps its rows out of menu search
    // for the mode.
    if let Some(menu) = by_id("view-menu") {
        if app.mode == Mode::Sprites {
            let _ = menu.set_attribute("hidden", "hidden");
            let _ = menu.remove_attribute("open");
            return;
        }
        let _ = menu.remove_attribute("hidden");
    }

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

/// A dropdown over the stage folds shut when the press is anywhere else, the
/// way every other dropdown on the platform does. Pressing a switch inside it
/// rebuilds the body, but the press itself lands inside the menu, so the menu
/// stays open while it is being worked.
pub fn bind_close() {
    let doc = crate::ui::document();
    on(doc.unchecked_ref(), "pointerdown", Scope::Global, |e| {
        let menu = match by_id("view-menu") {
            Some(n) => n,
            None => return,
        };
        if !menu.has_attribute("open") {
            return;
        }
        let target = e.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
        if menu.contains(target.as_ref()) {
            return;
        }
        let _ = menu.remove_attribute("open");
    });
}
