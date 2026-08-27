//! The menu search box: a place to type what you want changed rather than
//! remember which of eleven panels it lives in.
//!
//! Rows are rebuilt on every keystroke, so nothing here attaches a listener to
//! a row. One listener sits on the list and reads the row that was clicked,
//! which is what keeps a long session of typing from leaving a closure behind
//! per character.

use std::cell::{Cell, RefCell};

use wasm_bindgen::JsCast;
use web_sys::{Element, Event, HtmlElement, HtmlInputElement};

use crate::app::{self, Handle, Mode};
use crate::find::{Entry, Hit, Index};
use crate::ui::{by_id, clear, document, el, on, Scope};

/// How many rows the list shows. Long enough to hold the answer, short enough
/// to read without scrolling past the fold.
const LIMIT: usize = 12;

thread_local! {
    static INDEX: RefCell<Option<Index>> = const { RefCell::new(None) };
    /// The rows on screen, so the keyboard and the mouse agree on what is what.
    static HITS: RefCell<Vec<Hit>> = const { RefCell::new(Vec::new()) };
    static CURSOR: Cell<usize> = const { Cell::new(0) };
}

fn with_index<R>(f: impl FnOnce(&Index) -> R) -> R {
    INDEX.with(|slot| {
        let mut slot = slot.borrow_mut();
        f(slot.get_or_insert_with(Index::builtin))
    })
}

/// Whether the built index has a meaning table baked in. With none there is
/// nothing for the switch to do, so it is not offered.
pub fn has_meaning() -> bool {
    with_index(|i| i.has_terms())
}

pub fn mount(h: &Handle) {
    let (input, list) = match (by_id("find-box"), by_id("find-results")) {
        (Some(i), Some(l)) => match i.dyn_into::<HtmlInputElement>() {
            Ok(i) => (i, l),
            Err(_) => return,
        },
        _ => return,
    };

    if !has_meaning() {
        if let Some(node) = by_id("find-meaning-row") {
            let _ = node.set_attribute("hidden", "hidden");
        }
    }

    on(input.unchecked_ref(), "input", Scope::Global, |_| refresh());
    on(input.unchecked_ref(), "focus", Scope::Global, |_| refresh());

    if let Some(node) = by_id("find-meaning") {
        on(node.unchecked_ref(), "change", Scope::Global, |_| refresh());
    }

    {
        let h2 = h.clone();
        on(input.unchecked_ref(), "keydown", Scope::Global, move |e: Event| {
            let ke = match e.dyn_ref::<web_sys::KeyboardEvent>() {
                Some(k) => k,
                None => return,
            };
            let shown = HITS.with(|hits| hits.borrow().len());
            match ke.key().as_str() {
                "ArrowDown" => {
                    e.prevent_default();
                    step(1, shown);
                }
                "ArrowUp" => {
                    e.prevent_default();
                    step(-1, shown);
                }
                "Enter" => {
                    e.prevent_default();
                    let at = CURSOR.with(|c| c.get());
                    if let Some(hit) = HITS.with(|hits| hits.borrow().get(at).copied()) {
                        activate(&h2, hit.idx);
                    }
                }
                "Escape" => {
                    e.prevent_default();
                    close();
                }
                _ => {}
            }
        });
    }

    // One listener for every row there will ever be.
    {
        let h2 = h.clone();
        on(list.unchecked_ref(), "click", Scope::Global, move |e: Event| {
            let at = e
                .target()
                .and_then(|t| t.dyn_into::<Element>().ok())
                .and_then(|t| t.closest("[data-idx]").ok().flatten())
                .and_then(|row| row.get_attribute("data-idx"))
                .and_then(|v| v.parse::<usize>().ok());
            if let Some(idx) = at {
                activate(&h2, idx);
            }
        });
    }

    // A click anywhere else puts the list away. Pointerdown rather than click,
    // so the list is gone before whatever was clicked reacts to it.
    on(document().unchecked_ref(), "pointerdown", Scope::Global, |e: Event| {
        let inside = e
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|t| t.closest("#find").ok().flatten())
            .is_some();
        if !inside {
            close();
        }
    });
}

/// Puts the cursor in the box and shows the list. Bound to `/` so search is
/// one key away from anywhere in the page.
pub fn focus() {
    if let Some(node) = by_id("find-box") {
        if let Ok(input) = node.dyn_into::<HtmlInputElement>() {
            let _ = input.focus();
            input.select();
        }
    }
    refresh();
}

fn close() {
    if let Some(list) = by_id("find-results") {
        let _ = list.set_attribute("hidden", "hidden");
    }
    HITS.with(|hits| hits.borrow_mut().clear());
}

fn step(by: i32, shown: usize) {
    if shown == 0 {
        return;
    }
    let at = CURSOR.with(|c| c.get()) as i32;
    let next = (at + by).rem_euclid(shown as i32) as usize;
    CURSOR.with(|c| c.set(next));
    mark();
}

/// Moves the highlight without rebuilding the rows, so holding an arrow key
/// down does not rebuild the list once per repeat.
fn mark() {
    let list = match by_id("find-results") {
        Some(l) => l,
        None => return,
    };
    let at = CURSOR.with(|c| c.get());
    let rows = list.children();
    for i in 0..rows.length() {
        if let Some(row) = rows.item(i) {
            let class = if i as usize == at { "find-hit on" } else { "find-hit" };
            row.set_class_name(class);
        }
    }
}

fn refresh() {
    let (input, list) = match (by_id("find-box"), by_id("find-results")) {
        (Some(i), Some(l)) => (i, l),
        _ => return,
    };
    let query = input
        .dyn_into::<HtmlInputElement>()
        .map(|i| i.value())
        .unwrap_or_default();
    let by_meaning = by_id("find-meaning")
        .and_then(|n| n.dyn_into::<HtmlInputElement>().ok())
        .map(|i| i.checked())
        .unwrap_or(false);

    let rows = with_index(|index| {
        let hits = index.search(&query, by_meaning, LIMIT);
        hits.iter()
            .map(|hit| (*hit, index.entries[hit.idx].clone()))
            .collect::<Vec<_>>()
    });

    clear(&list);
    if rows.is_empty() {
        let _ = list.append_child(&el("li").class("find-empty").text("nothing by that name").get());
        let _ = list.remove_attribute("hidden");
        HITS.with(|h| h.borrow_mut().clear());
        return;
    }

    for (hit, entry) in &rows {
        let _ = list.append_child(&row_node(hit, entry));
    }
    HITS.with(|h| *h.borrow_mut() = rows.iter().map(|(hit, _)| *hit).collect());
    CURSOR.with(|c| c.set(0));
    let _ = list.remove_attribute("hidden");
    mark();
}

fn row_node(hit: &Hit, entry: &Entry) -> Element {
    let head = el("span")
        .class("find-label")
        .text(&entry.label)
        .maybe(if hit.by_meaning {
            // A row nobody's letters could have found needs to say why it is
            // here, or it reads as the search being broken.
            Some(el("span").class("find-why").text("meaning").get())
        } else {
            None
        })
        .get();
    el("li")
        .class("find-hit")
        .attr("data-idx", &hit.idx.to_string())
        .child(&head)
        .child(&el("span").class("find-path").text(&entry.path()).get())
        .get()
}

/// Shows whatever screen the entry lives on and points at it.
fn activate(h: &Handle, idx: usize) {
    let entry = match with_index(|i| i.entries.get(idx).cloned()) {
        Some(e) => e,
        None => return,
    };
    {
        let mut sh = h.borrow_mut();
        let sh = &mut *sh;
        if let Some(mode) = Mode::from_id(&entry.mode) {
            if sh.app.mode != mode {
                app::show_mode(sh, h, mode);
            }
            if let Some(tab) = app::tab_id_of(mode, &entry.tab) {
                if sh.app.ui.tab != tab {
                    app::show_tab(sh, h, tab);
                }
            }
        }
    }
    close();
    reveal(&entry.anchor);
}

/// Finds the control in the page, brings it into view and flashes it. The
/// flash is a CSS animation, so it has to be taken off and put back on with a
/// layout read in between or a second jump to the same row would not replay.
fn reveal(anchor: &str) {
    if anchor.is_empty() {
        return;
    }
    let selector = if let Some(id) = anchor.strip_prefix('#') {
        format!("#{id}")
    } else {
        format!("#panel-body [data-find=\"{anchor}\"]")
    };
    let node = match document().query_selector(&selector) {
        Ok(Some(n)) => n,
        _ => return,
    };
    if let Some(old) = document().query_selector(".found").ok().flatten() {
        let _ = old.class_list().remove_1("found");
    }
    if let Some(html) = node.dyn_ref::<HtmlElement>() {
        let _ = html.offset_width();
    }
    let _ = node.class_list().add_1("found");
    node.scroll_into_view();
    // The control itself, not the label around it, is what wants the keyboard.
    let inner = node
        .query_selector("input, select, button, textarea")
        .ok()
        .flatten()
        .unwrap_or(node);
    if let Some(html) = inner.dyn_ref::<HtmlElement>() {
        let _ = html.focus();
    }
}
