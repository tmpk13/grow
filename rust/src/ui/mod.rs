//! Minimal DOM helpers. Panels are generated from schemas rather than written
//! out as markup, so adding a parameter only means adding a schema entry.
//!
//! Event listeners are kept alive in one of three bags. A bag is emptied at the
//! same moment the nodes it belongs to are removed from the page, which is what
//! keeps a long session from leaking a closure per rebuild.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, Event, EventTarget, HtmlInputElement, HtmlSelectElement, Window};

pub mod art_panel;
pub mod build_panel;
pub mod color_wheel;
pub mod decode;
pub mod drive;
pub mod economy_panel;
pub mod experimental_panel;
pub mod find_box;
pub mod grid_editor;
pub mod land_panel;
pub mod map_panel;
pub mod materials_panel;
pub mod paint;
pub mod panel_resize;
pub mod people_panel;
pub mod prefs;
pub mod reset;
pub mod shading_panel;
pub mod species_panel;
pub mod sprite_drop;
pub mod sprite_store;
pub mod tech_panel;
pub mod restart_bar;
pub mod section_io;
pub mod view_menu;
pub mod world_panel;
pub mod zone_paint;

pub fn window() -> Window {
    web_sys::window().expect("no window")
}

pub fn document() -> Document {
    window().document().expect("no document")
}

/// Whether this is something with a keyboard and a pointer that can hit a
/// single pixel, which is the only place a shortcut is worth listing. A phone
/// answers no to both.
pub fn has_keyboard() -> bool {
    window()
        .match_media("(hover: hover) and (pointer: fine)")
        .ok()
        .flatten()
        .map(|q| q.matches())
        .unwrap_or(false)
}

pub fn by_id(id: &str) -> Option<Element> {
    document().get_element_by_id(id)
}

pub fn now() -> f64 {
    window().performance().map(|p| p.now()).unwrap_or(0.0)
}

/// Which lifetime a listener belongs to. Panel listeners die with the panel,
/// toolbar listeners with the toolbar, global ones last the session.
///
/// `List` is for the parts of a panel that are rebuilt on a timer: a roster
/// redrawn twice a second would otherwise add a closure per row every time and
/// never drop one. A panel that rebuilds interactive rows clears this scope at
/// the top of its redraw.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    Toolbar,
    Panel,
    List,
    /// The view menu in the side panel, which outlives a tab change but is
    /// rebuilt whenever one of its own switches changes what the rest show.
    View,
    /// The question the panel asks on the way out when a rebuild is waiting.
    Dialog,
    /// The chrome over the map for driving a person, which comes and goes
    /// with whoever is being driven rather than with a panel or a tab.
    Hud,
}

/// A listener, kept alive for as long as the node it is attached to.
type Listener = Closure<dyn FnMut(Event)>;

thread_local! {
    static GLOBAL_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
    static TOOLBAR_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
    static PANEL_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
    static LIST_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
    static VIEW_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
    static DIALOG_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
    static HUD_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
}

fn with_bag<R>(scope: Scope, f: impl FnOnce(&mut Vec<Listener>) -> R) -> R {
    match scope {
        Scope::Global => GLOBAL_BAG.with(|b| f(&mut b.borrow_mut())),
        Scope::Toolbar => TOOLBAR_BAG.with(|b| f(&mut b.borrow_mut())),
        Scope::Panel => PANEL_BAG.with(|b| f(&mut b.borrow_mut())),
        Scope::List => LIST_BAG.with(|b| f(&mut b.borrow_mut())),
        Scope::View => VIEW_BAG.with(|b| f(&mut b.borrow_mut())),
        Scope::Dialog => DIALOG_BAG.with(|b| f(&mut b.borrow_mut())),
        Scope::Hud => HUD_BAG.with(|b| f(&mut b.borrow_mut())),
    }
}

/// Drops every listener in a scope. Always clear the nodes first.
pub fn clear_scope(scope: Scope) {
    with_bag(scope, |bag| bag.clear());
}

pub fn on(target: &EventTarget, event: &str, scope: Scope, f: impl FnMut(Event) + 'static) {
    let closure = Listener::wrap(Box::new(f));
    target
        .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
        .expect("listener");
    with_bag(scope, |bag| bag.push(closure));
}

/// Same, but the listener asks the browser not to take the default action.
pub fn on_passive_false(target: &EventTarget, event: &str, scope: Scope, f: impl FnMut(Event) + 'static) {
    let closure = Listener::wrap(Box::new(f));
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_passive(false);
    target
        .add_event_listener_with_callback_and_add_event_listener_options(
            event,
            closure.as_ref().unchecked_ref(),
            &opts,
        )
        .expect("listener");
    with_bag(scope, |bag| bag.push(closure));
}

pub fn clear(node: &Element) {
    while let Some(child) = node.first_child() {
        let _ = node.remove_child(&child);
    }
}

/// A small builder so a panel reads as a tree rather than as a pile of
/// `create_element` calls.
pub struct E {
    pub node: Element,
}

pub fn el(tag: &str) -> E {
    E { node: document().create_element(tag).expect("element") }
}

impl E {
    pub fn class(self, c: &str) -> E {
        self.node.set_class_name(c);
        self
    }

    pub fn text(self, t: &str) -> E {
        self.node.set_text_content(Some(t));
        self
    }

    pub fn attr(self, k: &str, v: &str) -> E {
        let _ = self.node.set_attribute(k, v);
        self
    }

    pub fn style(self, k: &str, v: &str) -> E {
        if let Some(html) = self.node.dyn_ref::<web_sys::HtmlElement>() {
            let _ = html.style().set_property(k, v);
        }
        self
    }

    pub fn child(self, c: &Element) -> E {
        let _ = self.node.append_child(c);
        self
    }

    pub fn children(self, cs: Vec<Element>) -> E {
        for c in cs {
            let _ = self.node.append_child(&c);
        }
        self
    }

    pub fn maybe(self, c: Option<Element>) -> E {
        if let Some(c) = c {
            let _ = self.node.append_child(&c);
        }
        self
    }

    pub fn on(self, event: &str, scope: Scope, f: impl FnMut(Event) + 'static) -> E {
        on(self.node.unchecked_ref::<EventTarget>(), event, scope, f);
        self
    }

    pub fn get(self) -> Element {
        self.node
    }
}

/// Makes the children of `list` reorderable by dragging. Each child that can
/// be moved carries `data-drag-at` with its position; dropping one on another
/// calls `land(from, to)`.
///
/// Four listeners on the container rather than four per row: the list is
/// rebuilt whenever anything about the sheet changes, and a closure per row per
/// rebuild is a closure per row that never goes away.
pub fn reorder_by_drag(list: &Element, scope: Scope, land: impl Fn(usize, usize) + 'static) {
    let at_of = |e: &Event| -> Option<usize> {
        e.target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|t| t.closest("[data-drag-at]").ok().flatten())
            .and_then(|n| n.get_attribute("data-drag-at"))
            .and_then(|v| v.parse().ok())
    };

    on(list.unchecked_ref(), "dragstart", scope, move |e: Event| {
        let at = match at_of(&e) {
            Some(a) => a,
            None => return,
        };
        if let Some(dt) = e.dyn_ref::<web_sys::DragEvent>().and_then(|d| d.data_transfer()) {
            dt.set_effect_allowed("move");
            let _ = dt.set_data("text/plain", &at.to_string());
        }
    });

    // Without stopping the default, a drop never happens at all.
    let node = list.clone();
    on_passive_false(list.unchecked_ref(), "dragover", scope, move |e: Event| {
        e.prevent_default();
        let over = e
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|t| t.closest("[data-drag-at]").ok().flatten());
        mark_drop(&node, over.as_ref());
    });

    let node = list.clone();
    on(list.unchecked_ref(), "dragleave", scope, move |_| mark_drop(&node, None));

    let node = list.clone();
    on_passive_false(list.unchecked_ref(), "drop", scope, move |e: Event| {
        e.prevent_default();
        mark_drop(&node, None);
        let to = match at_of(&e) {
            Some(a) => a,
            None => return,
        };
        let from = e
            .dyn_ref::<web_sys::DragEvent>()
            .and_then(|d| d.data_transfer())
            .and_then(|dt| dt.get_data("text/plain").ok())
            .and_then(|v| v.parse::<usize>().ok());
        if let Some(from) = from {
            if from != to {
                land(from, to);
            }
        }
    });
}

/// Puts the drop marker on one row of a list and takes it off the rest.
fn mark_drop(list: &Element, row: Option<&Element>) {
    let rows = match list.query_selector_all("[data-drag-at]") {
        Ok(r) => r,
        Err(_) => return,
    };
    for i in 0..rows.length() {
        let node = match rows.item(i).and_then(|n| n.dyn_into::<Element>().ok()) {
            Some(n) => n,
            None => continue,
        };
        let want = row.is_some_and(|r| r.is_same_node(Some(&node)));
        let _ = if want {
            node.class_list().add_1("drop-here")
        } else {
            node.class_list().remove_1("drop-here")
        };
    }
}

pub fn append(parent: &Element, child: Element) {
    let _ = parent.append_child(&child);
}

pub fn input_el(kind: &str) -> HtmlInputElement {
    document()
        .create_element("input")
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap()
        .tap(|i| i.set_type(kind))
}

/// A tiny helper so an input can be configured inline where it is created.
pub trait Tap: Sized {
    fn tap(self, f: impl FnOnce(&Self)) -> Self {
        f(&self);
        self
    }
}

impl<T> Tap for T {}

pub fn value_of(e: &Event) -> String {
    e.target()
        .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        .map(|i| i.value())
        .unwrap_or_default()
}

pub fn select_value_of(e: &Event) -> String {
    e.target()
        .and_then(|t| t.dyn_into::<HtmlSelectElement>().ok())
        .map(|s| s.value())
        .unwrap_or_default()
}

fn num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        // Enough digits for the smallest step any field uses, without turning
        // 0.3 into 0.30000000000000004.
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub fn row(label: &str, control: Element, hint: Option<&str>) -> Element {
    // The stamp is what menu search jumps to. Every labeled control gets one
    // by passing through here, so the index can never name a row the page
    // does not have.
    el("label")
        .class("field")
        .attr("data-find", &slug(label))
        .child(&el("span").class("field-label").text(label).get())
        .child(&control)
        .maybe(hint.map(|h| el("span").class("field-hint").text(h).get()))
        .get()
}

pub struct NumOpts {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

/// A slider and a number box that stay in step with each other.
pub fn number_field(
    label: &str,
    value: f64,
    opts: NumOpts,
    hint: Option<&str>,
    on_input: impl FnMut(f64) + 'static,
) -> Element {
    let slider = input_el("range");
    let box_ = input_el("number");
    for input in [&slider, &box_] {
        let _ = input.set_attribute("min", &num(opts.min));
        let _ = input.set_attribute("max", &num(opts.max));
        let _ = input.set_attribute("step", &num(opts.step));
        input.set_value(&num(value));
    }
    box_.set_class_name("num");

    let sink = Rc::new(RefCell::new(on_input));
    {
        let other = box_.clone();
        let sink = sink.clone();
        on(slider.unchecked_ref(), "input", Scope::Panel, move |e| {
            let v: f64 = match value_of(&e).parse() {
                Ok(v) => v,
                Err(_) => return,
            };
            other.set_value(&num(v));
            (sink.borrow_mut())(v);
        });
    }
    {
        let other = slider.clone();
        let sink = sink.clone();
        on(box_.unchecked_ref(), "input", Scope::Panel, move |e| {
            let v: f64 = match value_of(&e).parse() {
                Ok(v) => v,
                Err(_) => return,
            };
            other.set_value(&num(v));
            (sink.borrow_mut())(v);
        });
    }

    let pair = el("span")
        .class("num-pair")
        .child(slider.unchecked_ref())
        .child(box_.unchecked_ref())
        .get();
    row(label, pair, hint)
}

/// A linked low/high pair; the high value is never allowed below the low one.
pub fn range_field(
    label: &str,
    lo: f64,
    hi: f64,
    opts: NumOpts,
    hint: Option<&str>,
    on_input: impl FnMut(f64, f64) + 'static,
) -> Element {
    let a = input_el("number");
    let b = input_el("number");
    for input in [&a, &b] {
        let _ = input.set_attribute("min", &num(opts.min));
        let _ = input.set_attribute("max", &num(opts.max));
        let _ = input.set_attribute("step", &num(opts.step));
        input.set_class_name("num");
    }
    a.set_value(&num(lo));
    b.set_value(&num(hi));

    let sink = Rc::new(RefCell::new(on_input));
    let emit = {
        let a = a.clone();
        let b = b.clone();
        let sink = sink.clone();
        move |_: Event| {
            let lo: f64 = match a.value().parse() {
                Ok(v) => v,
                Err(_) => return,
            };
            let mut hi: f64 = match b.value().parse() {
                Ok(v) => v,
                Err(_) => return,
            };
            if hi < lo {
                hi = lo;
                b.set_value(&num(hi));
            }
            (sink.borrow_mut())(lo, hi);
        }
    };
    on(a.unchecked_ref(), "input", Scope::Panel, emit.clone());
    on(b.unchecked_ref(), "input", Scope::Panel, emit);

    let pair = el("span")
        .class("range-pair")
        .child(a.unchecked_ref())
        .child(&el("span").text("to").get())
        .child(b.unchecked_ref())
        .get();
    row(label, pair, hint)
}

pub fn select_field(
    label: &str,
    value: &str,
    options: &[(String, String)],
    hint: Option<&str>,
    mut on_input: impl FnMut(String) + 'static,
) -> Element {
    let sel = document()
        .create_element("select")
        .unwrap()
        .dyn_into::<HtmlSelectElement>()
        .unwrap();
    for (v, l) in options {
        let opt = el("option").attr("value", v).text(l).get();
        if v == value {
            let _ = opt.set_attribute("selected", "selected");
        }
        let _ = sel.append_child(&opt);
    }
    sel.set_value(value);
    on(sel.unchecked_ref(), "change", Scope::Panel, move |e| {
        on_input(select_value_of(&e));
    });
    row(label, sel.unchecked_into(), hint)
}

/// A select with no field row around it, for the stage toolbar, which lays its
/// own controls out.
pub fn select_bare(
    value: &str,
    options: &[(String, String)],
    mut on_input: impl FnMut(String) + 'static,
) -> Element {
    let sel = document()
        .create_element("select")
        .unwrap()
        .dyn_into::<HtmlSelectElement>()
        .unwrap();
    for (v, l) in options {
        let opt = el("option").attr("value", v).text(l).get();
        if v == value {
            let _ = opt.set_attribute("selected", "selected");
        }
        let _ = sel.append_child(&opt);
    }
    sel.set_value(value);
    on(sel.unchecked_ref(), "change", Scope::Toolbar, move |e| {
        on_input(select_value_of(&e));
    });
    sel.unchecked_into()
}

pub fn bool_field(
    label: &str,
    value: bool,
    hint: Option<&str>,
    on_input: impl FnMut(bool) + 'static,
) -> Element {
    row(label, check_button(value, label, Scope::Panel, on_input), hint)
}

pub fn text_field(
    label: &str,
    value: &str,
    hint: Option<&str>,
    mut on_input: impl FnMut(String) + 'static,
) -> Element {
    let input = input_el("text");
    input.set_value(value);
    on(input.unchecked_ref(), "input", Scope::Panel, move |e| {
        on_input(value_of(&e));
    });
    row(label, input.unchecked_into(), hint)
}

pub fn color_field(
    label: &str,
    value: &str,
    hint: Option<&str>,
    mut on_input: impl FnMut(String) + 'static,
) -> Element {
    let input = input_el("color");
    input.set_value(value);
    on(input.unchecked_ref(), "input", Scope::Panel, move |e| {
        on_input(value_of(&e));
    });
    row(label, input.unchecked_into(), hint)
}

pub fn button(text: &str, scope: Scope, mut on_click: impl FnMut() + 'static) -> Element {
    el("button")
        .class("btn")
        .attr("type", "button")
        .attr("data-find", &slug(text))
        .text(text)
        .on("click", scope, move |_| on_click())
        .get()
}

/// Whether a switch is on. The state lives in the attribute rather than in a
/// property, so it survives the page being read back out of itself and a
/// listener can be attached to the node without holding the value.
pub fn pressed(node: &Element) -> bool {
    node.get_attribute("aria-pressed").as_deref() == Some("true")
}

/// A switch in the control column of a settings row: square, pressed in when
/// it is on, with a check drawn in it. The name is beside it already, so the
/// button carries none, and the label it belongs to says what it is for
/// anything reading the page rather than looking at it.
pub fn check_button(
    value: bool,
    label: &str,
    scope: Scope,
    mut on_input: impl FnMut(bool) + 'static,
) -> Element {
    let button = el("button")
        .class("btn toggle check")
        .attr("type", "button")
        .attr("aria-pressed", if value { "true" } else { "false" })
        .attr("aria-label", label)
        .get();
    let node = button.clone();
    on(button.unchecked_ref::<EventTarget>(), "click", scope, move |_| {
        let next = !pressed(&node);
        let _ = node.set_attribute("aria-pressed", if next { "true" } else { "false" });
        on_input(next);
    });
    button
}

/// A button that stays pressed. Used where a checkbox beside a word would be
/// a switch you have to read to know the state of: this one you can see.
pub fn toggle_button(
    text: &str,
    value: bool,
    scope: Scope,
    mut on_click: impl FnMut(bool) + 'static,
) -> Element {
    let button = el("button")
        .class("btn toggle")
        .attr("type", "button")
        .attr("aria-pressed", if value { "true" } else { "false" })
        .attr("data-find", &slug(text))
        .text(text)
        .get();
    let node = button.clone();
    on(button.unchecked_ref::<EventTarget>(), "click", scope, move |_| {
        let next = !pressed(&node);
        let _ = node.set_attribute("aria-pressed", if next { "true" } else { "false" });
        on_click(next);
    });
    button
}

pub fn danger_button(text: &str, scope: Scope, mut on_click: impl FnMut() + 'static) -> Element {
    el("button")
        .class("btn danger")
        .attr("type", "button")
        .attr("data-find", &slug(text))
        .text(text)
        .on("click", scope, move |_| on_click())
        .get()
}

/// Hands the browser something to save. The anchor has to be in the page for a
/// click on it to count, and is taken out again straight away.
/// Hands the browser a file made here. A blob rather than a data URL: an
/// archive of sheets runs to megabytes, and a URL that long is a string the
/// browser has to parse before it can save anything.
pub fn save_bytes(bytes: &[u8], mime: &str, filename: &str) {
    let array = js_sys::Array::new();
    // The copy is deliberate: a view straight onto the wasm heap is only valid
    // until the next allocation, and the blob outlives this call.
    array.push(&js_sys::Uint8Array::from(bytes).into());
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime);
    let blob = match web_sys::Blob::new_with_u8_array_sequence_and_options(&array, &options) {
        Ok(b) => b,
        Err(_) => return,
    };
    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
        Ok(u) => u,
        Err(_) => return,
    };
    download(&url, filename);
    let _ = web_sys::Url::revoke_object_url(&url);
}

pub fn download(href: &str, filename: &str) {
    let anchor = match document().create_element("a").ok().and_then(|a| {
        a.dyn_into::<web_sys::HtmlAnchorElement>().ok()
    }) {
        Some(a) => a,
        None => return,
    };
    anchor.set_href(href);
    anchor.set_download(filename);
    if let Some(body) = document().body() {
        let _ = body.append_child(&anchor);
        anchor.click();
        anchor.remove();
    }
}


pub fn note(text: &str) -> Element {
    el("p").class("note").text(text).get()
}

/// One titled block of a panel, folded by its own header.
///
/// Sections arrive folded: a panel is longer than a window and the map is what
/// most of the window is for, so a tab opens as a list of headings rather than
/// as a wall of controls. Which ones somebody has pulled open is kept with the
/// window preferences rather than on the node, because a panel is rebuilt
/// whole on most changes and a fold that lived on the node would shut again
/// every time.
///
/// A section that holds settings also carries the two buttons that take it
/// away with you, which read the section back out of the page it just drew.
pub fn section(title: &str, children: Vec<Element>) -> Element {
    let body = el("div").class("group-body").children(children).get();
    let head = el("summary").class("group-head").child(&el("h3").text(title).get()).get();
    if let Some(tools) = section_io::tools(title, &body) {
        let _ = head.append_child(&tools);
    }
    let node = el("details")
        .class("group")
        .attr("data-group", title)
        .child(&head)
        .child(&body)
        .get();
    if !prefs::Prefs::load().is_folded(title) {
        let _ = node.set_attribute("open", "open");
    }
    let key = title.to_string();
    let watched = node.clone();
    on(node.unchecked_ref(), "toggle", Scope::Panel, move |_| {
        // Setting `open` on the fresh node above queues a toggle per rebuild;
        // only an actual change of mind is worth a write.
        let folded = !watched.has_attribute("open");
        let mut prefs = prefs::Prefs::load();
        if prefs.is_folded(&key) != folded {
            prefs.set_folded(&key, folded);
            prefs.save();
        }
        sync_fold_all();
    });
    node
}

/// Every fold of the showing panel, pulled one way. Folding is the useful
/// direction - a panel is long and the map is what most of the window is for -
/// and unfolding is the way back, so one button does whichever is left.
pub fn bind_fold_all() {
    let btn = match by_id("btn-fold-groups") {
        Some(n) => n,
        None => return,
    };
    on(btn.unchecked_ref(), "click", Scope::Global, move |_| {
        let groups = panel_groups();
        let any_open = groups.iter().any(|g| g.has_attribute("open"));
        let mut prefs = prefs::Prefs::load();
        for g in groups {
            let _ = if any_open {
                g.remove_attribute("open")
            } else {
                g.set_attribute("open", "open")
            };
            if let Some(title) = g.get_attribute("data-group") {
                prefs.set_folded(&title, any_open);
            }
        }
        prefs.save();
        sync_fold_all();
    });
}

/// Keeps the button naming the direction it would pull, and hides it over a
/// panel with nothing to fold.
pub fn sync_fold_all() {
    let btn = match by_id("btn-fold-groups") {
        Some(n) => n,
        None => return,
    };
    let groups = panel_groups();
    if groups.is_empty() {
        let _ = btn.set_attribute("hidden", "hidden");
        return;
    }
    let _ = btn.remove_attribute("hidden");
    let any_open = groups.iter().any(|g| g.has_attribute("open"));
    btn.set_text_content(Some(if any_open { "Fold all" } else { "Unfold all" }));
}

fn panel_groups() -> Vec<Element> {
    let mut out = Vec::new();
    if let Ok(list) = document().query_selector_all("#panel-body details.group") {
        for i in 0..list.length() {
            if let Some(node) = list.item(i).and_then(|n| n.dyn_into::<Element>().ok()) {
                out.push(node);
            }
        }
    }
    out
}

/// The line over a row of chips that says what the chips are.
pub fn chip_head(text: &str) -> Element {
    el("h4").class("chip-head").text(text).get()
}

pub fn stat(key: &str, value: &str) -> Element {
    el("div")
        .class("stat")
        .child(&el("span").class("stat-key").text(key).get())
        .child(&el("span").class("stat-val").text(value).get())
        .get()
}

pub fn bar(kind: &str, value: f64) -> Element {
    let width = format!("{}%", (value.clamp(0.0, 1.0) * 100.0).round());
    el("span")
        .class("bar")
        .child(
            &el("span")
                .class(&format!("bar-fill {kind}"))
                .style("width", &width)
                .get(),
        )
        .get()
}

// ---- panel helpers -------------------------------------------------------
//
// Every panel control ends in the same three steps: borrow the app, record what
// it is about to change, apply the change. These wrap that, so a panel reads as
// a list of parameters and every one of them is undoable without the panel
// having to say so.
//
// A control somebody holds - a slider, a text field, a color picker - fires
// while it is being held, so those coalesce into one step per burst. A control
// somebody presses does not.

use crate::app::{App, Handle};
pub use crate::util::{file_name, slug};

pub fn app_num(
    h: &Handle,
    label: &str,
    value: f64,
    opts: NumOpts,
    hint: Option<&str>,
    apply: impl Fn(&mut App, f64) + 'static,
) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    number_field(label, value, opts, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, true);
        apply(&mut sh.app, v);
    })
}

/// A number the running world was built from. Changing it does not rebuild
/// anything: it is noted, starred in the panel and waits for Apply, because a
/// slider that restarts a settlement at every value it passes through is a
/// slider nobody can hold.
#[allow(clippy::too_many_arguments)]
pub fn app_restart_num(
    h: &Handle,
    which: crate::app::Restart,
    label: &str,
    value: f64,
    opts: NumOpts,
    hint: Option<&str>,
    apply: impl Fn(&mut App, f64) + 'static,
) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    let field = number_field(label, value, opts, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, true);
        apply(&mut sh.app, v);
        sh.app.needs_restart(which, &key);
        crate::ui::restart_bar::sync(&sh.app);
    });
    let _ = field.set_attribute("data-restart", "");
    field
}

pub fn app_range(
    h: &Handle,
    label: &str,
    lo: f64,
    hi: f64,
    opts: NumOpts,
    hint: Option<&str>,
    apply: impl Fn(&mut App, f64, f64) + 'static,
) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    range_field(label, lo, hi, opts, hint, move |lo, hi| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, true);
        apply(&mut sh.app, lo, hi);
    })
}

pub fn app_bool(
    h: &Handle,
    label: &str,
    value: bool,
    hint: Option<&str>,
    apply: impl Fn(&mut App, bool) + 'static,
) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    bool_field(label, value, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, false);
        apply(&mut sh.app, v);
    })
}

pub fn app_text(
    h: &Handle,
    label: &str,
    value: &str,
    hint: Option<&str>,
    apply: impl Fn(&mut App, &str) + 'static,
) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    text_field(label, value, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, true);
        apply(&mut sh.app, &v);
    })
}

pub fn app_color(
    h: &Handle,
    label: &str,
    value: &str,
    hint: Option<&str>,
    apply: impl Fn(&mut App, &str) + 'static,
) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    color_field(label, value, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, true);
        apply(&mut sh.app, &v);
    })
}

pub fn app_select(
    h: &Handle,
    label: &str,
    value: &str,
    options: &[(String, String)],
    hint: Option<&str>,
    apply: impl Fn(&mut App, &str) + 'static,
) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    select_field(label, value, options, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, false);
        apply(&mut sh.app, &v);
    })
}

pub fn app_button(h: &Handle, label: &str, apply: impl Fn(&mut App) + 'static) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    button(label, Scope::Panel, move || {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, false);
        apply(&mut sh.app);
    })
}

pub fn app_danger_button(h: &Handle, label: &str, apply: impl Fn(&mut App) + 'static) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    danger_button(label, Scope::Panel, move || {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, false);
        apply(&mut sh.app);
    })
}

/// The undo pair lives in the top bar, outside every panel, because a step
/// covers the whole project rather than whichever editor is open. Nothing
/// rebuilds it, so it is told when the history moves.
pub fn sync_undo_buttons(app: &App) {
    for (id, enabled) in [
        ("btn-undo", app.history.can_undo()),
        ("btn-redo", app.history.can_redo()),
    ] {
        if let Some(node) = by_id(id) {
            let _ = if enabled {
                node.remove_attribute("disabled")
            } else {
                node.set_attribute("disabled", "disabled")
            };
        }
    }
}

pub fn btn_row(children: Vec<Element>) -> Element {
    el("div").class("btn-row").children(children).get()
}

/// A row of buttons, one per colony, that sets which town every settlement
/// panel is reporting on. Panels that show one town's books all start with
/// this, so switching once switches everywhere.
pub fn colony_picker(app: &App, h: &Handle) -> Option<Element> {
    let civ = app.settlement.as_ref()?;
    if civ.colonies.len() < 2 {
        return None;
    }
    let focus = civ.focus;
    let row = el("div").class("chips").get();
    for (i, colony) in civ.colonies.iter().enumerate() {
        let h2 = h.clone();
        let class = if i == focus { "chip active" } else { "chip" };
        let label = if colony.abandoned {
            format!("{} (empty)", colony.name)
        } else {
            format!("{} {}", colony.name, colony.population)
        };
        let chip = el("button")
            .class(class)
            .attr("type", "button")
            .text(&label)
            .on("click", Scope::Panel, move |_| {
                let mut sh = h2.borrow_mut();
                if let Some(civ) = &mut sh.app.settlement {
                    civ.focus = i;
                }
                sh.app.rebuild_panel = true;
            })
            .get();
        let _ = row.append_child(&chip);
    }
    Some(
        el("div")
            .class("chip-block")
            .child(&chip_head("Which town the panels show"))
            .child(&row)
            .get(),
    )
}

/// The sampling boxes as select options.
pub fn sampler_options(app: &App) -> Vec<(String, String)> {
    app.state
        .materials
        .samplers
        .iter()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect()
}
