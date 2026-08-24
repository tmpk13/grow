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

pub mod build_panel;
pub mod economy_panel;
pub mod grid_editor;
pub mod land_panel;
pub mod materials_panel;
pub mod people_panel;
pub mod shading_panel;
pub mod species_panel;
pub mod tech_panel;
pub mod world_panel;

pub fn window() -> Window {
    web_sys::window().expect("no window")
}

pub fn document() -> Document {
    window().document().expect("no document")
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
}

/// A listener, kept alive for as long as the node it is attached to.
type Listener = Closure<dyn FnMut(Event)>;

thread_local! {
    static GLOBAL_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
    static TOOLBAR_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
    static PANEL_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
    static LIST_BAG: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
}

fn with_bag<R>(scope: Scope, f: impl FnOnce(&mut Vec<Listener>) -> R) -> R {
    match scope {
        Scope::Global => GLOBAL_BAG.with(|b| f(&mut b.borrow_mut())),
        Scope::Toolbar => TOOLBAR_BAG.with(|b| f(&mut b.borrow_mut())),
        Scope::Panel => PANEL_BAG.with(|b| f(&mut b.borrow_mut())),
        Scope::List => LIST_BAG.with(|b| f(&mut b.borrow_mut())),
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

pub fn checked_of(e: &Event) -> bool {
    e.target()
        .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        .map(|i| i.checked())
        .unwrap_or(false)
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
    el("label")
        .class("field")
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

pub fn bool_field(
    label: &str,
    value: bool,
    hint: Option<&str>,
    mut on_input: impl FnMut(bool) + 'static,
) -> Element {
    let box_ = input_el("checkbox");
    box_.set_checked(value);
    on(box_.unchecked_ref(), "change", Scope::Panel, move |e| {
        on_input(checked_of(&e));
    });
    row(label, box_.unchecked_into(), hint)
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
        .text(text)
        .on("click", scope, move |_| on_click())
        .get()
}

pub fn danger_button(text: &str, scope: Scope, mut on_click: impl FnMut() + 'static) -> Element {
    el("button")
        .class("btn danger")
        .attr("type", "button")
        .text(text)
        .on("click", scope, move |_| on_click())
        .get()
}

pub fn note(text: &str) -> Element {
    el("p").class("note").text(text).get()
}

pub fn section(title: &str, children: Vec<Element>) -> Element {
    el("section")
        .class("group")
        .child(
            &el("header")
                .class("group-head")
                .child(&el("h3").text(title).get())
                .get(),
        )
        .child(&el("div").class("group-body").children(children).get())
        .get()
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
// Every panel control ends in the same two steps: borrow the app, apply the
// change. These wrap that so a panel reads as a list of parameters.

use crate::app::{App, Handle};

pub fn app_num(
    h: &Handle,
    label: &str,
    value: f64,
    opts: NumOpts,
    hint: Option<&str>,
    apply: impl Fn(&mut App, f64) + 'static,
) -> Element {
    let h2 = h.clone();
    number_field(label, value, opts, hint, move |v| {
        let mut sh = h2.borrow_mut();
        apply(&mut sh.app, v);
    })
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
    range_field(label, lo, hi, opts, hint, move |lo, hi| {
        let mut sh = h2.borrow_mut();
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
    bool_field(label, value, hint, move |v| {
        let mut sh = h2.borrow_mut();
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
    text_field(label, value, hint, move |v| {
        let mut sh = h2.borrow_mut();
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
    color_field(label, value, hint, move |v| {
        let mut sh = h2.borrow_mut();
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
    select_field(label, value, options, hint, move |v| {
        let mut sh = h2.borrow_mut();
        apply(&mut sh.app, &v);
    })
}

pub fn app_button(h: &Handle, label: &str, apply: impl Fn(&mut App) + 'static) -> Element {
    let h2 = h.clone();
    button(label, Scope::Panel, move || {
        let mut sh = h2.borrow_mut();
        apply(&mut sh.app);
    })
}

pub fn app_danger_button(h: &Handle, label: &str, apply: impl Fn(&mut App) + 'static) -> Element {
    let h2 = h.clone();
    danger_button(label, Scope::Panel, move || {
        let mut sh = h2.borrow_mut();
        apply(&mut sh.app);
    })
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
    Some(row)
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
