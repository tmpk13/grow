//! Application shell: the two modes, their tabs, the stage toolbars and the
//! frame loop.
//!
//! The plant lab and the settlement are two views onto the same project: the
//! species and sampling boxes authored in the lab are what grows on the
//! settlement map and what its buildings are drawn from.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, Event, HtmlCanvasElement, HtmlInputElement};

use crate::civ::civ_render::Detail;
use crate::civ::people::Profession;
use crate::civ::resources::Res;
use crate::civ::settlement::Settlement;
use crate::plant::Scratch;
use crate::render::Viewport;
use crate::sim::{Env, Sim};
use crate::state::{State, STORAGE_KEY};
use crate::ui::{self, by_id, clear, clear_scope, document, el, on, on_passive_false, window, Scope};
use crate::util::{clamp, hex_to_packed};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Lab,
    Settlement,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Pencil,
    Eraser,
    Fill,
    Pick,
}

pub struct UiState {
    pub tab: &'static str,
    pub selected_sampler: String,
    pub selected_species: String,
    pub brush_color: u32,
    pub tool: Tool,
    pub mirror_x: bool,
    pub shade_preview_sampler: String,
    pub shade_preview_tones: i32,
    pub shade_preview_core: f64,
}

pub struct App {
    pub state: State,
    pub sim: Sim,
    pub settlement: Option<Settlement>,
    pub viewport: Viewport,
    pub mode: Mode,
    pub ui: UiState,
    /// Shared by the species preview, which rasterizes outside the sim.
    pub env: Env,
    pub scratch: Scratch,
    pub next_uid: u32,
    pub rebuild_panel: bool,
    pub redraw_panel: bool,
    /// The warmup blocks the thread, so it is deferred by one frame to let the
    /// note paint first.
    pub pending_bootstrap: bool,
    pub pending_civ_reset: bool,
    pub save_deadline: Option<f64>,
    pub fps: f64,
    pub accumulator: f64,
    pub last_ts: f64,
}

impl App {
    pub fn active_tick_hz(&self) -> f64 {
        self.sim_settings().tick_hz.max(1.0)
    }

    pub fn sim_settings(&self) -> crate::civ::config::SimSettings {
        match self.mode {
            Mode::Lab => self.state.sim,
            Mode::Settlement => self.state.civ.sim,
        }
    }

    pub fn set_running(&mut self, running: bool) {
        match self.mode {
            Mode::Lab => self.state.sim.running = running,
            Mode::Settlement => self.state.civ.sim.running = running,
        }
    }

    pub fn set_speed(&mut self, speed: f64) {
        match self.mode {
            Mode::Lab => self.state.sim.speed = speed,
            Mode::Settlement => self.state.civ.sim.speed = speed,
        }
    }

    pub fn uid(&mut self, prefix: &str) -> String {
        self.next_uid = self.next_uid.wrapping_add(1);
        crate::util::uid(prefix, self.next_uid)
    }

    // ---- change notifications -------------------------------------------

    pub fn materials_changed(&mut self) {
        self.state.materials.touch();
        self.env.invalidate();
        self.sim.env.invalidate();
        self.sim.mark_all_dirty();
        if let Some(civ) = &mut self.settlement {
            civ.invalidate_sprites();
            civ.mark_all_dirty();
            civ.plant_sim.env.invalidate();
        }
        self.request_save();
    }

    pub fn shading_changed(&mut self) {
        self.sim.mark_all_dirty();
        if let Some(civ) = &mut self.settlement {
            civ.mark_all_dirty();
        }
        self.request_save();
    }

    pub fn species_changed(&mut self) {
        self.env.invalidate();
        self.sim.env.invalidate();
        self.sim.mark_all_dirty();
        if let Some(civ) = &mut self.settlement {
            civ.plant_sim.env.invalidate();
            civ.mark_all_dirty();
        }
        self.redraw_panel = true;
        self.request_save();
    }

    pub fn world_changed(&mut self) {
        self.sim.world_cfg = self.state.world.clone();
        self.sim.reset(self.state.seed);
        self.viewport.fit(&self.sim.world);
        self.request_save();
    }

    pub fn repaint_background(&mut self) {
        self.sim.world_cfg = self.state.world.clone();
        self.sim.buffer_dirty = true;
        self.request_save();
    }

    pub fn restart(&mut self) {
        self.sim.reset(self.state.seed);
        self.viewport.fit(&self.sim.world);
    }

    pub fn civ_repaint(&mut self) {
        if let Some(civ) = &mut self.settlement {
            civ.invalidate_sprites();
        }
        self.request_save();
    }

    /// Rebuilds the map and the settlers on it. The heavy part runs on the next
    /// frame so the note has a chance to paint.
    pub fn civ_restart(&mut self) {
        self.set_note("growing the wilderness...");
        self.pending_civ_reset = true;
        self.pending_bootstrap = true;
    }

    pub fn set_note(&self, text: &str) {
        if let Some(node) = by_id("save-note") {
            node.set_text_content(Some(text));
        }
    }

    pub fn request_save(&mut self) {
        self.save_deadline = Some(ui::now() + 600.0);
    }

    pub fn rebuild_panel(&mut self) {
        self.rebuild_panel = true;
    }

    pub fn save_now(&mut self) {
        let ok = save_local(&self.state);
        let stamp = js_sys::Date::new_0().to_locale_time_string("en-US");
        self.set_note(&if ok {
            format!("saved {}", String::from(stamp))
        } else {
            "save failed".to_string()
        });
    }
}

pub trait Panel {
    fn redraw(&mut self, _app: &mut App) {}
    fn tick(&mut self, _app: &mut App, _dt: f64) {}
    /// Selection and view state a panel keeps for itself. A listener cannot
    /// reach into the concrete panel type through the shell, so the three
    /// things the register needs to be told travel through here; every other
    /// panel ignores them.
    fn select(&mut self, _id: u32) {}
    fn set_sort(&mut self, _sort: u8) {}
    fn toggle_dead(&mut self) {}
}

/// A panel with nothing to update between frames.
pub struct StaticPanel;
impl Panel for StaticPanel {}

pub struct Shell {
    pub app: App,
    pub panel: Option<Box<dyn Panel>>,
}

pub type Handle = Rc<RefCell<Shell>>;

type Builder = fn(&Element, &mut App, &Handle) -> Box<dyn Panel>;

struct TabDef {
    id: &'static str,
    label: &'static str,
    build: Builder,
}

const LAB_TABS: &[TabDef] = &[
    TabDef { id: "materials", label: "Materials", build: ui::materials_panel::build },
    TabDef { id: "shading", label: "Shading", build: ui::shading_panel::build },
    TabDef { id: "species", label: "Species", build: ui::species_panel::build },
    TabDef { id: "world", label: "World", build: ui::world_panel::build },
];

const CIV_TABS: &[TabDef] = &[
    TabDef { id: "land", label: "Land", build: ui::land_panel::build },
    TabDef { id: "people", label: "People", build: ui::people_panel::build },
    TabDef { id: "build", label: "Build", build: ui::build_panel::build },
    TabDef { id: "economy", label: "Economy", build: ui::economy_panel::build },
    TabDef { id: "tech", label: "Tech", build: ui::tech_panel::build },
];

fn tabs_for(mode: Mode) -> &'static [TabDef] {
    match mode {
        Mode::Lab => LAB_TABS,
        Mode::Settlement => CIV_TABS,
    }
}

// ---- storage -------------------------------------------------------------

fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

pub fn save_local(state: &State) -> bool {
    match storage() {
        Some(store) => store.set_item(STORAGE_KEY, &state.to_json()).is_ok(),
        None => false,
    }
}

pub fn load_local() -> Option<State> {
    let raw = storage()?.get_item(STORAGE_KEY).ok()??;
    match State::from_json(&raw) {
        Ok(state) => Some(state),
        Err(err) => {
            web_sys::console::warn_1(&JsValue::from_str(&format!("load failed: {err}")));
            None
        }
    }
}

// ---- boot ----------------------------------------------------------------

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&JsValue::from_str(&format!("{info}")));
    }));

    let state = load_local().unwrap_or_default();
    let sim = Sim::new(&state, state.world.clone());
    let canvas = by_id("world-canvas")
        .expect("world-canvas")
        .dyn_into::<HtmlCanvasElement>()?;
    let viewport = Viewport::new(canvas.clone());

    let ui_state = UiState {
        tab: "materials",
        selected_sampler: state.materials.samplers.first().map(|s| s.id.clone()).unwrap_or_default(),
        selected_species: state.species.first().map(|s| s.id.clone()).unwrap_or_default(),
        brush_color: hex_to_packed("#7ab55c"),
        tool: Tool::Pencil,
        mirror_x: false,
        shade_preview_sampler: state
            .materials
            .samplers
            .first()
            .map(|s| s.id.clone())
            .unwrap_or_default(),
        shade_preview_tones: 5,
        shade_preview_core: 4.0,
    };

    let app = App {
        state,
        sim,
        settlement: None,
        viewport,
        mode: Mode::Lab,
        ui: ui_state,
        env: Env::default(),
        scratch: Scratch::default(),
        next_uid: (js_sys::Math::random() * 1e6) as u32,
        rebuild_panel: false,
        redraw_panel: false,
        pending_bootstrap: false,
        pending_civ_reset: false,
        save_deadline: None,
        fps: 60.0,
        accumulator: 0.0,
        last_ts: ui::now(),
    };

    let handle: Handle = Rc::new(RefCell::new(Shell { app, panel: None }));

    bind_canvas(&handle, &canvas);
    bind_keys(&handle);
    bind_project_actions(&handle);
    bind_resize(&handle, &canvas);

    {
        let mut sh = handle.borrow_mut();
        let sh = &mut *sh;
        show_mode(sh, &handle, Mode::Lab);
        sh.app.viewport.fit(&sh.app.sim.world);
    }

    start_frame_loop(handle);
    Ok(())
}

// ---- modes and tabs ------------------------------------------------------

pub fn show_mode(sh: &mut Shell, h: &Handle, mode: Mode) {
    sh.app.mode = mode;
    let modes_node = match by_id("modes") {
        Some(n) => n,
        None => return,
    };
    clear(&modes_node);
    clear_scope(Scope::Toolbar);
    for (id, label) in [(Mode::Lab, "Plant lab"), (Mode::Settlement, "Settlement")] {
        let h2 = h.clone();
        let class = if id == mode { "mode active" } else { "mode" };
        let btn = el("button")
            .class(class)
            .attr("type", "button")
            .text(label)
            .on("click", Scope::Toolbar, move |_| {
                let mut sh = h2.borrow_mut();
                let sh = &mut *sh;
                show_mode(sh, &h2, id);
            })
            .get();
        let _ = modes_node.append_child(&btn);
    }

    if mode == Mode::Settlement && sh.app.settlement.is_none() {
        sh.app.set_note("growing the wilderness...");
        sh.app.settlement = Some(Settlement::new(&sh.app.state));
        sh.app.pending_bootstrap = true;
    }

    build_toolbar(sh, h);
    let first = tabs_for(mode)[0].id;
    show_tab(sh, h, first);
    let world_fit = active_world_size(&sh.app);
    sh.app.viewport.fit(&world_fit);
}

fn active_world_size(app: &App) -> crate::world::World {
    match (app.mode, &app.settlement) {
        (Mode::Settlement, Some(civ)) => civ.world().clone(),
        _ => app.sim.world.clone(),
    }
}

pub fn show_tab(sh: &mut Shell, h: &Handle, id: &'static str) {
    let tabs = tabs_for(sh.app.mode);
    let tab = tabs.iter().find(|t| t.id == id).unwrap_or(&tabs[0]);
    sh.app.ui.tab = tab.id;

    let tabs_node = match by_id("tabs") {
        Some(n) => n,
        None => return,
    };
    clear(&tabs_node);
    for t in tabs {
        let h2 = h.clone();
        let tid = t.id;
        let class = if t.id == tab.id { "tab active" } else { "tab" };
        let btn = el("button")
            .class(class)
            .attr("type", "button")
            .text(t.label)
            .on("click", Scope::Toolbar, move |_| {
                let mut sh = h2.borrow_mut();
                let sh = &mut *sh;
                show_tab(sh, &h2, tid);
            })
            .get();
        let _ = tabs_node.append_child(&btn);
    }

    let body = match by_id("panel-body") {
        Some(n) => n,
        None => return,
    };
    sh.panel = None;
    clear(&body);
    clear_scope(Scope::Panel);
    clear_scope(Scope::List);
    sh.panel = Some((tab.build)(&body, &mut sh.app, h));
    sh.app.rebuild_panel = false;
}

// ---- stage toolbar -------------------------------------------------------

fn build_toolbar(sh: &mut Shell, h: &Handle) {
    let toolbar = match by_id("stage-toolbar") {
        Some(n) => n,
        None => return,
    };
    clear(&toolbar);

    let cfg = sh.app.sim_settings();
    let mut controls: Vec<Element> = Vec::new();

    let play = el("button")
        .class("btn")
        .attr("id", "btn-play")
        .attr("type", "button")
        .text(if cfg.running { "Pause" } else { "Play" })
        .get();
    {
        let h2 = h.clone();
        let play_node = play.clone();
        on(play.unchecked_ref(), "click", Scope::Toolbar, move |_| {
            let mut sh = h2.borrow_mut();
            let running = !sh.app.sim_settings().running;
            sh.app.set_running(running);
            play_node.set_text_content(Some(if running { "Pause" } else { "Play" }));
            sh.app.request_save();
        });
    }
    controls.push(play);

    controls.push(ui::button("Step", Scope::Toolbar, {
        let h2 = h.clone();
        move || {
            let mut sh = h2.borrow_mut();
            let dt = 1.0 / sh.app.active_tick_hz();
            step_active(&mut sh.app, dt);
        }
    }));

    if sh.app.mode == Mode::Settlement {
        controls.push(ui::button("New settlers", Scope::Toolbar, {
            let h2 = h.clone();
            move || {
                let mut sh = h2.borrow_mut();
                sh.app.civ_restart();
            }
        }));
        controls.push(ui::button("New land", Scope::Toolbar, {
            let h2 = h.clone();
            move || {
                let mut sh = h2.borrow_mut();
                sh.app.state.civ.seed = (js_sys::Math::random() * 1e9) as u32;
                sh.app.civ_restart();
                sh.app.rebuild_panel();
            }
        }));
    } else {
        controls.push(ui::button("Restart", Scope::Toolbar, {
            let h2 = h.clone();
            move || {
                let mut sh = h2.borrow_mut();
                sh.app.restart();
            }
        }));
    }

    // Speed
    let speed = ui::input_el("range");
    let _ = speed.set_attribute("min", "0.25");
    let _ = speed.set_attribute("max", "32");
    let _ = speed.set_attribute("step", "0.25");
    speed.set_value(&format!("{}", cfg.speed));
    let speed_label = el("span").class("readout").text(&format!("{}x", cfg.speed)).get();
    {
        let h2 = h.clone();
        let label = speed_label.clone();
        on(speed.unchecked_ref(), "input", Scope::Toolbar, move |e| {
            let v: f64 = ui::value_of(&e).parse().unwrap_or(1.0);
            let mut sh = h2.borrow_mut();
            sh.app.set_speed(v);
            label.set_text_content(Some(&format!("{v}x")));
            sh.app.request_save();
        });
    }
    controls.push(
        el("label")
            .class("inline")
            .child(&el("span").text("Speed").get())
            .child(speed.unchecked_ref())
            .child(&speed_label)
            .get(),
    );

    // Zoom
    let zoom = ui::input_el("range");
    let _ = zoom.set_attribute("min", "0.5");
    let _ = zoom.set_attribute("max", "16");
    let _ = zoom.set_attribute("step", "0.25");
    zoom.set_attribute("id", "zoom-input").ok();
    zoom.set_value(&format!("{}", sh.app.viewport.zoom));
    let zoom_label = el("span")
        .class("readout")
        .attr("id", "zoom-readout")
        .text(&format!("{:.2}x", sh.app.viewport.zoom))
        .get();
    {
        let h2 = h.clone();
        on(zoom.unchecked_ref(), "input", Scope::Toolbar, move |e| {
            let target: f64 = ui::value_of(&e).parse().unwrap_or(1.0);
            let mut sh = h2.borrow_mut();
            let r = sh.app.viewport.canvas.get_bounding_client_rect();
            let (cx, cy) = (r.left() + r.width() / 2.0, r.top() + r.height() / 2.0);
            let factor = target / sh.app.viewport.zoom;
            sh.app.viewport.zoom_at(cx, cy, factor);
            sync_zoom(&sh.app);
        });
    }
    controls.push(
        el("label")
            .class("inline")
            .child(&el("span").text("Zoom").get())
            .child(zoom.unchecked_ref())
            .child(&zoom_label)
            .get(),
    );

    controls.push(ui::button("Fit", Scope::Toolbar, {
        let h2 = h.clone();
        move || {
            let mut sh = h2.borrow_mut();
            let world = active_world_size(&sh.app);
            sh.app.viewport.fit(&world);
            sync_zoom(&sh.app);
        }
    }));

    let grid = ui::input_el("checkbox");
    grid.set_checked(sh.app.viewport.show_grid);
    {
        let h2 = h.clone();
        on(grid.unchecked_ref(), "change", Scope::Toolbar, move |e| {
            h2.borrow_mut().app.viewport.show_grid = ui::checked_of(&e);
        });
    }
    controls.push(
        el("label")
            .class("inline")
            .child(&el("span").text("Grid").get())
            .child(grid.unchecked_ref())
            .get(),
    );

    let occ = ui::input_el("checkbox");
    occ.set_checked(sh.app.viewport.show_occupancy);
    {
        let h2 = h.clone();
        on(occ.unchecked_ref(), "change", Scope::Toolbar, move |e| {
            h2.borrow_mut().app.viewport.show_occupancy = ui::checked_of(&e);
        });
    }
    controls.push(
        el("label")
            .class("inline")
            .child(&el("span").text("Occupancy").get())
            .child(occ.unchecked_ref())
            .get(),
    );

    if sh.app.mode == Mode::Settlement {
        let labels = ui::input_el("checkbox");
        labels.set_checked(sh.app.state.civ.view.labels);
        {
            let h2 = h.clone();
            on(labels.unchecked_ref(), "change", Scope::Toolbar, move |e| {
                let mut sh = h2.borrow_mut();
                sh.app.state.civ.view.labels = ui::checked_of(&e);
                sh.app.request_save();
            });
        }
        controls.push(
            el("label")
                .class("inline")
                .child(&el("span").text("Labels").get())
                .child(labels.unchecked_ref())
                .get(),
        );
    }

    let _ = toolbar.append_child(&el("div").class("toolbar-row").children(controls).get());
}

fn sync_zoom(app: &App) {
    if let Some(node) = by_id("zoom-input") {
        if let Ok(input) = node.dyn_into::<HtmlInputElement>() {
            input.set_value(&format!("{}", clamp(app.viewport.zoom, 0.5, 16.0)));
        }
    }
    if let Some(node) = by_id("zoom-readout") {
        node.set_text_content(Some(&format!("{:.2}x", app.viewport.zoom)));
    }
}

// ---- canvas interaction --------------------------------------------------

fn bind_canvas(h: &Handle, canvas: &HtmlCanvasElement) {
    {
        let h2 = h.clone();
        on_passive_false(canvas.unchecked_ref(), "wheel", Scope::Global, move |e: Event| {
            e.prevent_default();
            let we = e.dyn_ref::<web_sys::WheelEvent>().unwrap();
            let mut sh = h2.borrow_mut();
            let factor = if we.delta_y() < 0.0 { 1.12 } else { 1.0 / 1.12 };
            sh.app.viewport.zoom_at(we.client_x() as f64, we.client_y() as f64, factor);
            sync_zoom(&sh.app);
        });
    }

    let drag = Rc::new(RefCell::new((false, 0.0f64, 0.0f64)));
    {
        let drag = drag.clone();
        let canvas2 = canvas.clone();
        on(canvas.unchecked_ref(), "pointerdown", Scope::Global, move |e: Event| {
            let pe = e.dyn_ref::<web_sys::PointerEvent>().unwrap();
            *drag.borrow_mut() = (true, pe.client_x() as f64, pe.client_y() as f64);
            let _ = canvas2.set_pointer_capture(pe.pointer_id());
        });
    }
    {
        let drag = drag.clone();
        let h2 = h.clone();
        on(canvas.unchecked_ref(), "pointermove", Scope::Global, move |e: Event| {
            let mut d = drag.borrow_mut();
            if !d.0 {
                return;
            }
            let pe = e.dyn_ref::<web_sys::PointerEvent>().unwrap();
            let (x, y) = (pe.client_x() as f64, pe.client_y() as f64);
            h2.borrow_mut().app.viewport.pan(x - d.1, y - d.2);
            d.1 = x;
            d.2 = y;
        });
    }
    for event in ["pointerup", "pointercancel"] {
        let drag = drag.clone();
        on(canvas.unchecked_ref(), event, Scope::Global, move |_| {
            drag.borrow_mut().0 = false;
        });
    }
}

fn bind_keys(h: &Handle) {
    let h2 = h.clone();
    on(window().unchecked_ref(), "keydown", Scope::Global, move |e: Event| {
        let ke = e.dyn_ref::<web_sys::KeyboardEvent>().unwrap();
        if let Some(target) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) {
            if matches!(target.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") {
                return;
            }
        }
        let mut sh = h2.borrow_mut();
        match ke.code().as_str() {
            "Space" => {
                e.prevent_default();
                if let Some(btn) = by_id("btn-play") {
                    if let Ok(btn) = btn.dyn_into::<web_sys::HtmlElement>() {
                        drop(sh);
                        btn.click();
                    }
                }
            }
            _ => match ke.key().as_str() {
                "." => {
                    let dt = 1.0 / sh.app.active_tick_hz();
                    step_active(&mut sh.app, dt);
                }
                "f" => {
                    let world = active_world_size(&sh.app);
                    sh.app.viewport.fit(&world);
                    sync_zoom(&sh.app);
                }
                "m" => {
                    let next = if sh.app.mode == Mode::Lab {
                        Mode::Settlement
                    } else {
                        Mode::Lab
                    };
                    let sh = &mut *sh;
                    show_mode(sh, &h2, next);
                }
                _ => {}
            },
        }
    });
}

fn bind_resize(h: &Handle, canvas: &HtmlCanvasElement) {
    let h2 = h.clone();
    let closure = Closure::wrap(Box::new(move |_: JsValue| {
        h2.borrow_mut().app.viewport.resize();
    }) as Box<dyn FnMut(JsValue)>);
    if let Ok(observer) = web_sys::ResizeObserver::new(closure.as_ref().unchecked_ref()) {
        if let Some(parent) = canvas.parent_element() {
            observer.observe(&parent);
        }
        // The observer has to outlive this function, and it lives as long as
        // the page does.
        std::mem::forget(observer);
    }
    closure.forget();
}

fn bind_project_actions(h: &Handle) {
    if let Some(btn) = by_id("btn-new") {
        let h2 = h.clone();
        on(btn.unchecked_ref(), "click", Scope::Global, move |_| {
            let mut sh = h2.borrow_mut();
            let sh = &mut *sh;
            sh.app.state = State::new();
            sh.app.env.invalidate();
            sh.app.sim.env.invalidate();
            sh.app.sim.world_cfg = sh.app.state.world.clone();
            sh.app.sim.reset(sh.app.state.seed);
            sh.app.settlement = None;
            sh.app.viewport.fit(&sh.app.sim.world);
            reset_selection(&mut sh.app);
            show_mode(sh, &h2, Mode::Lab);
            sh.app.request_save();
        });
    }

    if let Some(btn) = by_id("btn-export") {
        let h2 = h.clone();
        on(btn.unchecked_ref(), "click", Scope::Global, move |_| {
            let sh = h2.borrow();
            let json = sh.app.state.to_pretty_json();
            export_json(&json);
        });
    }

    if let Some(node) = by_id("file-import") {
        let input = node.dyn_into::<HtmlInputElement>().unwrap();
        let h2 = h.clone();
        let input2 = input.clone();
        on(input.unchecked_ref(), "change", Scope::Global, move |_| {
            let file = match input2.files().and_then(|f| f.get(0)) {
                Some(f) => f,
                None => return,
            };
            let reader = web_sys::FileReader::new().unwrap();
            let h3 = h2.clone();
            let reader2 = reader.clone();
            let name = file.name();
            let input3 = input2.clone();
            let onload = Closure::wrap(Box::new(move |_: Event| {
                let text = reader2.result().ok().and_then(|v| v.as_string()).unwrap_or_default();
                let mut sh = h3.borrow_mut();
                let sh = &mut *sh;
                match State::from_json(&text) {
                    Ok(state) => {
                        sh.app.state = state;
                        sh.app.env.invalidate();
                        sh.app.sim.env.invalidate();
                        sh.app.sim.world_cfg = sh.app.state.world.clone();
                        sh.app.sim.reset(sh.app.state.seed);
                        sh.app.settlement = None;
                        sh.app.viewport.fit(&sh.app.sim.world);
                        reset_selection(&mut sh.app);
                        show_mode(sh, &h3, Mode::Lab);
                        sh.app.set_note(&format!("imported {name}"));
                        sh.app.request_save();
                    }
                    Err(err) => sh.app.set_note(&format!("import failed: {err}")),
                }
                input3.set_value("");
            }) as Box<dyn FnMut(Event)>);
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            let _ = reader.read_as_text(&file);
            onload.forget();
        });
    }
}

fn reset_selection(app: &mut App) {
    app.ui.selected_sampler = app
        .state
        .materials
        .samplers
        .first()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    app.ui.selected_species = app.state.species.first().map(|s| s.id.clone()).unwrap_or_default();
    app.ui.shade_preview_sampler = app.ui.selected_sampler.clone();
}

fn export_json(json: &str) {
    let array = js_sys::Array::new();
    array.push(&JsValue::from_str(json));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/json");
    let blob = match web_sys::Blob::new_with_str_sequence_and_options(&array, &options) {
        Ok(b) => b,
        Err(_) => return,
    };
    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
        Ok(u) => u,
        Err(_) => return,
    };
    let anchor = document()
        .create_element("a")
        .unwrap()
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .unwrap();
    anchor.set_href(&url);
    anchor.set_download("grow-project.json");
    let body = document().body().unwrap();
    let _ = body.append_child(&anchor);
    anchor.click();
    anchor.remove();
    let _ = web_sys::Url::revoke_object_url(&url);
}

// ---- frame loop ----------------------------------------------------------

fn step_active(app: &mut App, dt: f64) {
    match app.mode {
        Mode::Lab => {
            let App { sim, state, .. } = app;
            sim.step(state, dt, None);
        }
        Mode::Settlement => {
            if let Some(civ) = &mut app.settlement {
                civ.step(&app.state, dt);
            }
        }
    }
}

fn start_frame_loop(h: Handle) {
    let cb = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let cb2 = cb.clone();
    *cb.borrow_mut() = Some(Closure::wrap(Box::new(move |ts: f64| {
        frame(&h, ts);
        request_frame(cb2.borrow().as_ref().unwrap());
    }) as Box<dyn FnMut(f64)>));
    request_frame(cb.borrow().as_ref().unwrap());
    // The loop owns itself for the life of the page.
    std::mem::forget(cb);
}

fn request_frame(cb: &Closure<dyn FnMut(f64)>) {
    let _ = window().request_animation_frame(cb.as_ref().unchecked_ref());
}

fn frame(h: &Handle, ts: f64) {
    let mut sh = h.borrow_mut();
    let sh = &mut *sh;

    let dt_real = ((ts - sh.app.last_ts) / 1000.0).clamp(0.0, 0.1);
    sh.app.last_ts = ts;
    sh.app.fps = sh.app.fps * 0.9 + (1.0 / dt_real.max(1e-3)) * 0.1;

    // A settlement is founded on the frame after the note is shown, because the
    // wilderness warmup blocks the thread for a moment.
    if sh.app.pending_bootstrap {
        sh.app.pending_bootstrap = false;
        if sh.app.pending_civ_reset {
            sh.app.pending_civ_reset = false;
            let seed = sh.app.state.civ.seed;
            if let Some(civ) = &mut sh.app.settlement {
                civ.reset(&sh.app.state, seed);
            } else {
                sh.app.settlement = Some(Settlement::new(&sh.app.state));
            }
        }
        if let Some(civ) = &mut sh.app.settlement {
            civ.bootstrap(&sh.app.state);
            let world = civ.world().clone();
            let name = civ.name.clone();
            sh.app.viewport.fit(&world);
            sh.app.set_note(&format!("{name} founded"));
            sh.app.redraw_panel = true;
            sh.app.request_save();
        }
    }

    let cfg = sh.app.sim_settings();
    let civ_ready = sh.app.settlement.as_ref().is_some_and(|c| c.ready);
    if cfg.running && (sh.app.mode == Mode::Lab || civ_ready) {
        let step_dt = 1.0 / cfg.tick_hz.max(1.0);
        sh.app.accumulator += dt_real * cfg.speed;
        let mut steps = 0;
        while sh.app.accumulator >= step_dt && steps < 400 {
            step_active(&mut sh.app, step_dt);
            sh.app.accumulator -= step_dt;
            steps += 1;
        }
        if sh.app.accumulator > 2.0 {
            sh.app.accumulator = 0.0;
        }
    } else {
        sh.app.accumulator = 0.0;
    }

    draw(&mut sh.app, cfg.raster_budget);

    if let Some(panel) = &mut sh.panel {
        panel.tick(&mut sh.app, dt_real);
    }
    update_status(&sh.app);

    if sh.app.redraw_panel {
        sh.app.redraw_panel = false;
        if let Some(panel) = &mut sh.panel {
            panel.redraw(&mut sh.app);
        }
    }
    if sh.app.rebuild_panel {
        let tab = sh.app.ui.tab;
        show_tab(sh, h, tab);
    }
    if let Some(at) = sh.app.save_deadline {
        if ui::now() >= at {
            sh.app.save_deadline = None;
            sh.app.save_now();
        }
    }
}

fn draw(app: &mut App, budget: usize) {
    match app.mode {
        Mode::Lab => {
            {
                let App { sim, state, .. } = app;
                sim.process_raster_queue(state, budget);
                if sim.buffer_dirty {
                    sim.composite(state);
                }
            }
            let world = app.sim.world.clone();
            let buffer = std::mem::take(&mut app.sim.buffer);
            app.viewport.present(&world, &buffer);
            app.sim.buffer = buffer;
            if app.viewport.show_occupancy {
                app.viewport.draw_occupancy(&world);
            }
            if app.viewport.show_grid {
                app.viewport.draw_grid(&world);
            }
            app.viewport.finish();
        }
        Mode::Settlement => {
            let mut civ = match app.settlement.take() {
                Some(c) => c,
                None => return,
            };
            // The camera decides what gets drawn and how much of it: the
            // settlement never composites more than the window can show, and
            // sheds detail as the zoom pulls back.
            let world = civ.world().clone();
            civ.view = app.viewport.visible_rect(&world);
            let detail = Detail::for_zoom(app.viewport.zoom, app.state.civ.view.detail_zoom);
            if detail != civ.detail {
                // Contact shadows are baked into the cached ground and are one
                // of the things detail drops, so the cache has to be rebuilt
                // rather than waiting out its refresh timer.
                civ.ground_dirty = true;
                civ.detail = detail;
            }
            if civ.ready {
                civ.process_raster_queue(&app.state, budget);
                // People move every frame, so the visible band is recomposited
                // every frame rather than only when something is marked dirty.
                civ.composite(&app.state);
            }
            let region = civ.view;
            let buffer = std::mem::take(&mut civ.buffer);
            app.viewport.present_region(&world, &buffer, region);
            civ.buffer = buffer;
            app.viewport.draw_civ_overlay(&civ, &app.state);
            app.viewport.draw_colony_labels(&civ);
            if app.viewport.show_occupancy {
                app.viewport.draw_occupancy(&world);
            }
            if app.viewport.show_grid {
                app.viewport.draw_grid(&world);
            }
            app.viewport.finish();
            app.settlement = Some(civ);
        }
    }
}

fn update_status(app: &App) {
    let status = match by_id("statusbar") {
        Some(n) => n,
        None => return,
    };
    let text = match app.mode {
        Mode::Settlement => match &app.settlement {
            Some(civ) if civ.ready => settlement_status(app, civ),
            _ => "growing the wilderness...".to_string(),
        },
        Mode::Lab => {
            let s = app.sim.stats();
            let mut parts = vec![
                format!("tick {}", s.ticks),
                format!("sim time {:.1}", s.time),
                format!("plants {}", s.total),
                format!("queue {}", app.sim.raster_queue.len()),
                format!("{:.0} fps", app.fps),
            ];
            for sp in &app.state.species {
                parts.push(format!(
                    "{}: {}",
                    sp.name,
                    s.per_species.get(&sp.id).copied().unwrap_or(0)
                ));
            }
            parts.join("   ")
        }
    };
    status.set_text_content(Some(&text));
}

fn settlement_status(app: &App, civ: &Settlement) -> String {
    let s = civ.stats(&app.state);
    let clock = format!(
        "{:02}:{:02}",
        (s.day_fraction * 24.0).floor() as i32,
        ((s.day_fraction * 24.0 * 60.0) % 60.0).floor() as i32
    );
    let jobs = s
        .professions
        .iter()
        .filter(|(p, _)| *p != Profession::Child)
        .map(|(p, n)| format!("{} {}", p.label(), n))
        .collect::<Vec<_>>()
        .join(" ");
    let towns = if s.colonies.len() > 1 {
        format!("{} towns", s.colonies.len())
    } else {
        s.name.clone()
    };
    [
        towns,
        format!("day {} {}", s.day, clock),
        format!("people {} ({} children)", s.population, s.children),
        format!("beds {}", s.housing),
        format!(
            "built {}{}",
            s.buildings,
            if s.sites > 0 { format!(" +{}", s.sites) } else { String::new() }
        ),
        format!("food {}", civ.total_stock()[Res::Food as usize].round()),
        format!("coin {}", s.coin.round()),
        format!("tech {}/{}", s.known, s.techs),
        if s.boats > 0 { format!("boats {}", s.boats) } else { String::new() },
        jobs,
        format!("{} detail", civ.detail.label()),
        format!("{:.0} fps", app.fps),
    ]
    .iter()
    .filter(|part| !part.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join("   ")
}
