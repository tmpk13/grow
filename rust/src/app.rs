//! Application shell: the two modes, their tabs, the stage toolbars and the
//! frame loop.
//!
//! The plant lab and the settlement are two views onto the same project: the
//! species and sampling boxes authored in the lab are what grows on the
//! settlement map and what its buildings are drawn from.

use std::cell::{Cell, RefCell};
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
use crate::undo::History;
use crate::ui::paint::Surface;
use crate::ui::{self, by_id, clear, clear_scope, document, el, on, on_passive_false, window, Scope};
use crate::util::{clamp, hex_to_packed, EMPTY_COLOR};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Lab,
    Sprites,
    Settlement,
}

pub const MODES: [(Mode, &str); 3] = [
    (Mode::Lab, "Plant lab"),
    (Mode::Sprites, "Sprite editor"),
    (Mode::Settlement, "Settlement"),
];

impl UiState {
    /// The selection as corners in order, clamped to the sheet, or None when
    /// there is no selection or it has been dragged down to nothing.
    pub fn marquee_rect(&self, w: i32, h: i32) -> Option<(i32, i32, i32, i32)> {
        let (ax, ay, bx, by) = self.marquee?;
        let x0 = ax.min(bx).clamp(0, w - 1);
        let y0 = ay.min(by).clamp(0, h - 1);
        let x1 = ax.max(bx).clamp(0, w - 1);
        let y1 = ay.max(by).clamp(0, h - 1);
        if x1 < x0 || y1 < y0 {
            return None;
        }
        Some((x0, y0, x1, y1))
    }
}

impl Mode {
    /// A short name that survives being written to a file, unlike the enum
    /// and unlike the label, which is prose and free to change.
    pub fn id(self) -> &'static str {
        match self {
            Mode::Lab => "lab",
            Mode::Sprites => "sprites",
            Mode::Settlement => "settlement",
        }
    }

    pub fn from_id(id: &str) -> Option<Mode> {
        MODES.iter().map(|(m, _)| *m).find(|m| m.id() == id)
    }

    pub fn label(self) -> &'static str {
        MODES.iter().find(|(m, _)| *m == self).map(|(_, l)| *l).unwrap_or("")
    }
}

/// Which running world a setting belongs to. A change to one of these is not
/// acted on until it is applied, so the two have to be told apart.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Restart {
    Lab = 0,
    Civ = 1,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Pencil,
    Eraser,
    Fill,
    Pick,
    /// Drags out a rectangle rather than drawing. What is inside it is what
    /// the nudges and the clear act on, instead of the whole cel.
    Select,
}

pub struct UiState {
    pub tab: &'static str,
    pub selected_sampler: String,
    pub selected_species: String,
    pub brush_color: u32,
    /// Hue, saturation and value of the brush, kept beside the packed color
    /// because a gray has no hue to read back out of it.
    pub brush_hsv: (f64, f64, f64),
    /// Show the color wheel rather than only the plain color box.
    pub use_wheel: bool,
    pub tool: Tool,
    pub mirror_x: bool,
    /// The sheet, layer and frame the sprite editor is pointed at.
    pub selected_sheet: String,
    pub sheet_layer: usize,
    pub sheet_frame: i32,
    /// Draw the frame before this one behind it, faint.
    pub onion: bool,
    pub playing: bool,
    pub play_time: f64,
    /// The stage picks settlers up rather than dragging the map. Kept here
    /// rather than in the project: it is how somebody is using the map right
    /// now, not something about the map.
    pub move_people: bool,
    /// The corners a marquee was dragged between, in sheet pixels, kept raw so
    /// the anchor survives the drag. Read through `marquee_rect`.
    pub marquee: Option<(i32, i32, i32, i32)>,
    /// What is typed into the picture panel's own search box, and its two
    /// switches. How somebody is using that panel, not anything about the
    /// project.
    pub made_search: String,
    pub made_all: bool,
    pub made_meaning: bool,
    /// Sheets left out of the next zip. Empty means all of them, so a project
    /// that has just gained a sheet includes it without anybody saying so.
    pub zip_skip: Vec<String>,
    /// Put every frame in the zip as its own image as well as the strip.
    pub zip_frames: bool,
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
    /// When the status line was last rewritten. Reading it costs a walk over
    /// every settler and building, which is not worth doing once a frame for a
    /// line of text nobody can read changing that fast.
    pub status_at: f64,
    pub accumulator: f64,
    pub last_ts: f64,
    /// What the two pixel editors can put back.
    pub history: History,
    /// Settings changed since the world they describe was built. Sorted, so
    /// the bar lists them in the same order twice running.
    pub pending: std::collections::BTreeSet<(Restart, String)>,
    /// What each running world was built from, for telling a changed setting
    /// from one that has been changed back.
    built: [String; 2],
}

impl App {
    pub fn active_tick_hz(&self) -> f64 {
        self.sim_settings().tick_hz.max(1.0)
    }

    /// The sprite editor runs no simulation, so it borrows the lab's settings
    /// for the fields the shell reads unconditionally and never writes to them.
    pub fn sim_settings(&self) -> crate::civ::config::SimSettings {
        match self.mode {
            Mode::Settlement => self.state.civ.sim,
            _ => self.state.sim,
        }
    }

    pub fn set_running(&mut self, running: bool) {
        match self.mode {
            Mode::Lab => self.state.sim.running = running,
            Mode::Sprites => {}
            Mode::Settlement => self.state.civ.sim.running = running,
        }
    }

    pub fn set_speed(&mut self, speed: f64) {
        match self.mode {
            Mode::Lab => self.state.sim.speed = speed,
            Mode::Sprites => {}
            Mode::Settlement => self.state.civ.sim.speed = speed,
        }
    }

    /// The sheet the editor is pointed at, and its size.
    pub fn sheet_dims(&self) -> Option<(i32, i32)> {
        self.state.art.find(&self.ui.selected_sheet).map(|s| (s.w, s.h))
    }

    pub fn uid(&mut self, prefix: &str) -> String {
        self.next_uid = self.next_uid.wrapping_add(1);
        crate::util::uid(prefix, self.next_uid)
    }

    // ---- undo -----------------------------------------------------------

    /// Records the project before a control changes it. `key` names the
    /// control; `coalesce` is for the ones a person holds rather than presses,
    /// so a slider drag is one step rather than one a frame.
    pub fn record(&mut self, key: &str, coalesce: bool) {
        let now = ui::now();
        self.history.record(&self.state, key, coalesce, now);
        ui::sync_undo_buttons(self);
    }

    pub fn undo(&mut self) {
        self.take_step(false);
    }

    pub fn redo(&mut self) {
        self.take_step(true);
    }

    fn take_step(&mut self, forward: bool) {
        let before = Marks::of(&self.state);
        let mut history = std::mem::take(&mut self.history);
        let moved = if forward {
            history.redo(&mut self.state)
        } else {
            history.undo(&mut self.state)
        };
        self.history = history;
        ui::sync_undo_buttons(self);
        if !moved {
            self.set_note(if forward { "nothing to redo" } else { "nothing to undo" });
            return;
        }
        self.after_restore(before);
    }

    /// Puts everything that reads the project back in step with it. A restored
    /// project can differ anywhere, so everything cached from it is dropped;
    /// what is not done unconditionally is starting a simulation again, which
    /// only happens if the step actually moved what a simulation is built on.
    fn after_restore(&mut self, before: Marks) {
        let now = Marks::of(&self.state);
        self.state.materials.invalidate();
        self.env.invalidate();
        self.sim.env.invalidate();
        self.sim.mark_all_dirty();
        self.sim.world_cfg = self.state.world.clone();
        if now.world != before.world || now.seed != before.seed {
            self.sim.reset(self.state.seed);
            self.viewport.fit(&self.sim.world);
        } else {
            self.sim.buffer_dirty = true;
        }
        if let Some(civ) = &mut self.settlement {
            civ.invalidate_sprites();
            civ.mark_all_dirty();
            civ.plant_sim.env.invalidate();
        }
        if now.civ_world != before.civ_world || now.civ_seed != before.civ_seed {
            self.civ_restart();
        }
        self.clamp_selection();
        // A step back may have put a starred setting back the way the running
        // world was built, in which case there is nothing waiting any more.
        self.sync_pending();
        ui::restart_bar::sync(self);
        self.rebuild_panel();
        self.request_save();
    }

    /// Keeps every selection the panels hold pointing at something that is
    /// still there, which a step back may have taken away.
    pub fn clamp_selection(&mut self) {
        if self.state.materials.find(&self.ui.selected_sampler).is_none() {
            self.ui.selected_sampler = self
                .state
                .materials
                .samplers
                .first()
                .map(|s| s.id.clone())
                .unwrap_or_default();
        }
        if self.state.find_species(&self.ui.selected_species).is_none() {
            self.ui.selected_species =
                self.state.species.first().map(|s| s.id.clone()).unwrap_or_default();
        }
        if self.state.art.find(&self.ui.selected_sheet).is_none() {
            self.ui.selected_sheet =
                self.state.art.sheets.first().map(|s| s.id.clone()).unwrap_or_default();
        }
        if let Some(sheet) = self.state.art.find(&self.ui.selected_sheet) {
            self.ui.sheet_layer = self.ui.sheet_layer.min(sheet.layers.len().saturating_sub(1));
            self.ui.sheet_frame = self.ui.sheet_frame.clamp(0, sheet.frame_count() - 1);
        }
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
        self.mark_built(Restart::Lab);
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

    /// A settler clip changed. The pixels every cached sprite was scaled from
    /// are gone, so the cache is dropped and the revision moves on; the
    /// settlement recomposites every frame, so nothing else has to be asked.
    pub fn sprites_changed(&mut self) {
        self.state.civ.sprites.touch();
        if let Some(civ) = &mut self.settlement {
            civ.invalidate_sprites();
        }
        self.request_save();
    }

    /// A sheet in the sprite editor changed. Only the panel reads sheets
    /// directly; a settler drawn from one is drawn from the clip that was built
    /// out of it, which is left alone until it is rebuilt on purpose.
    pub fn art_changed(&mut self) {
        self.redraw_panel = true;
        self.request_save();
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
        self.mark_built(Restart::Civ);
    }

    /// Notes that a setting the running world was built from has moved. The
    /// world is left alone: a slider that restarts a settlement on every value
    /// it passes through is a slider nobody can use.
    pub fn needs_restart(&mut self, which: Restart, label: &str) {
        self.pending.insert((which, label.to_string()));
        self.request_save();
        self.sync_pending();
    }

    /// Drops what now matches the world that is running. Undoing a change is
    /// the case that matters: the star it left behind would otherwise stand
    /// for a difference that is no longer there.
    pub fn sync_pending(&mut self) {
        for which in [Restart::Lab, Restart::Civ] {
            if self.restart_key(which) == self.built[which as usize] {
                self.pending.retain(|(w, _)| *w != which);
            }
        }
    }

    /// Which settings are waiting, of the kind this mode would restart.
    pub fn waiting(&self, which: Restart) -> Vec<&str> {
        self.pending
            .iter()
            .filter(|(w, _)| *w == which)
            .map(|(_, label)| label.as_str())
            .collect()
    }

    pub fn restart_target(&self) -> Restart {
        match self.mode {
            Mode::Settlement => Restart::Civ,
            _ => Restart::Lab,
        }
    }

    /// Rebuilds whatever is waiting on it.
    pub fn apply_restarts(&mut self) {
        let kinds: Vec<Restart> = [Restart::Lab, Restart::Civ]
            .into_iter()
            .filter(|w| self.pending.iter().any(|(k, _)| k == w))
            .collect();
        self.pending.clear();
        for which in kinds {
            match which {
                Restart::Lab => self.world_changed(),
                Restart::Civ => self.civ_restart(),
            }
        }
    }

    /// Puts the settings back the way the running world was built with, which
    /// is the other way out of a change nobody wants to sit through.
    pub fn discard_restarts(&mut self) {
        self.record("discard changes", false);
        for which in [Restart::Lab, Restart::Civ] {
            let built = self.built[which as usize].clone();
            match which {
                Restart::Lab => {
                    if let Ok(w) = serde_json::from_str(&built) {
                        self.state.world = w;
                    }
                }
                Restart::Civ => {
                    if let Ok((world, terrain)) = serde_json::from_str(&built) {
                        self.state.civ.world = world;
                        self.state.civ.terrain = terrain;
                    }
                }
            }
        }
        self.pending.clear();
        self.request_save();
    }

    /// What the running world was built from, so a later value can be told
    /// apart from it. Only the settings a rebuild depends on: a sky color or a
    /// label switch changes the picture without changing the world.
    fn restart_key(&self, which: Restart) -> String {
        let out = match which {
            Restart::Lab => serde_json::to_string(&self.state.world),
            Restart::Civ => {
                serde_json::to_string(&(&self.state.civ.world, &self.state.civ.terrain))
            }
        };
        out.unwrap_or_default()
    }

    pub fn mark_built(&mut self, which: Restart) {
        self.built[which as usize] = self.restart_key(which);
        self.pending.retain(|(w, _)| *w != which);
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
        if ui::prefs::Prefs::load().keep_sprites {
            ui::sprite_store::keep(&self.state.art);
        }
        let ok = save_local(&self.state);
        let stamp = js_sys::Date::new_0().to_locale_time_string("en-US");
        self.set_note(&if ok {
            format!("saved {}", String::from(stamp))
        } else {
            "save failed".to_string()
        });
    }
}

/// The parts of a project a simulation is built on. A step that leaves these
/// alone is a step nothing has to be restarted for, which is what keeps undoing
/// a brush stroke from throwing away the settlement.
struct Marks {
    world: crate::world::WorldConfig,
    seed: u32,
    civ_world: crate::world::WorldConfig,
    civ_seed: u32,
}

impl Marks {
    fn of(state: &State) -> Marks {
        Marks {
            world: state.world.clone(),
            seed: state.seed,
            civ_world: state.civ.world.clone(),
            civ_seed: state.civ.seed,
        }
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

/// The top of the zoom slider. A map is looked at from far enough away to see
/// a town; a sprite is looked at a pixel at a time.
const ZOOM_MAX_WORLD: f64 = 16.0;

/// The two squares of the checker behind a sheet, drawn into the buffer so they
/// scale with the art.
const CHECKER_LIGHT: u32 = crate::util::pack_rgba(26, 31, 38, 255);
const CHECKER_DARK: u32 = crate::util::pack_rgba(20, 25, 32, 255);

const ZOOM_MAX_SPRITE: f64 = 48.0;

/// The dashes of the selection outline. Bright enough to find over any art,
/// and not a color a plant or a settler is ever drawn in.
const MARQUEE_COLOR: u32 = crate::util::pack_rgba(255, 255, 255, 255);

/// How often the status line is rewritten. Fast enough to read as live, slow
/// enough that the walk over the settlement it costs does not land in a frame.
const STATUS_INTERVAL_MS: f64 = 200.0;

const LAB_TABS: &[TabDef] = &[
    TabDef { id: "materials", label: "Materials", build: ui::materials_panel::build },
    TabDef { id: "shading", label: "Shading", build: ui::shading_panel::build },
    TabDef { id: "species", label: "Species", build: ui::species_panel::build },
    TabDef { id: "world", label: "World", build: ui::world_panel::build },
];

const SPRITE_TABS: &[TabDef] = &[
    TabDef { id: "draw", label: "Draw", build: ui::art_panel::build_draw },
    TabDef { id: "sheet", label: "Sheet", build: ui::art_panel::build_sheet },
];

const CIV_TABS: &[TabDef] = &[
    TabDef { id: "land", label: "Land", build: ui::land_panel::build },
    TabDef { id: "people", label: "People", build: ui::people_panel::build },
    TabDef { id: "build", label: "Build", build: ui::build_panel::build },
    TabDef { id: "economy", label: "Economy", build: ui::economy_panel::build },
    TabDef { id: "tech", label: "Tech", build: ui::tech_panel::build },
];

/// The tab of `mode` with this id, as the static string the tab machinery
/// wants. Menu search carries ids as text, having read them out of the page.
pub fn tab_id_of(mode: Mode, id: &str) -> Option<&'static str> {
    tabs_for(mode).iter().find(|t| t.id == id).map(|t| t.id)
}

fn tabs_for(mode: Mode) -> &'static [TabDef] {
    match mode {
        Mode::Lab => LAB_TABS,
        Mode::Sprites => SPRITE_TABS,
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
        brush_hsv: crate::util::packed_to_hsv(hex_to_packed("#7ab55c"), 0.0),
        use_wheel: false,
        tool: Tool::Pencil,
        mirror_x: false,
        selected_sheet: state.art.sheets.first().map(|s| s.id.clone()).unwrap_or_default(),
        sheet_layer: 0,
        sheet_frame: 0,
        onion: true,
        playing: false,
        play_time: 0.0,
        move_people: false,
        marquee: None,
        made_search: String::new(),
        made_all: false,
        made_meaning: false,
        zip_skip: Vec::new(),
        zip_frames: false,
        shade_preview_sampler: state
            .materials
            .samplers
            .first()
            .map(|s| s.id.clone())
            .unwrap_or_default(),
        shade_preview_tones: 5,
        shade_preview_core: 4.0,
    };

    let mut app = App {
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
        status_at: 0.0,
        accumulator: 0.0,
        last_ts: ui::now(),
        history: History::default(),
        pending: Default::default(),
        built: Default::default(),
    };
    app.built = [app.restart_key(Restart::Lab), app.restart_key(Restart::Civ)];

    let handle: Handle = Rc::new(RefCell::new(Shell { app, panel: None }));

    bind_view_actions(&handle);
    ui::view_menu::bind_fold();
    ui::restart_bar::mount(&handle);
    ui::find_box::mount(&handle);
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
    for (id, label) in MODES {
        let h2 = h.clone();
        let class = if id == mode { "mode active" } else { "mode" };
        let btn = el("button")
            .class(class)
            .attr("type", "button")
            .attr("data-mode", id.id())
            .text(label)
            .on("click", Scope::Toolbar, move |_| {
                ui::restart_bar::leaving(
                    &h2,
                    Rc::new(move |h: &Handle| {
                        let mut sh = h.borrow_mut();
                        let sh = &mut *sh;
                        show_mode(sh, h, id);
                    }),
                );
            })
            .get();
        let _ = modes_node.append_child(&btn);
    }

    if mode == Mode::Settlement && sh.app.settlement.is_none() {
        sh.app.set_note("growing the wilderness...");
        sh.app.settlement = Some(Settlement::new(&sh.app.state));
        sh.app.pending_bootstrap = true;
    }

    if let Some(body) = document().body() {
        let list = body.class_list();
        let _ = if mode == Mode::Sprites {
            list.add_1("painting")
        } else {
            list.remove_1("painting")
        };
    }
    if mode != Mode::Sprites && sh.app.ui.tool == Tool::Select {
        // The marquee belongs to the sheet, and the sampling grid has no row
        // of tools to change it back with.
        sh.app.ui.tool = Tool::Pencil;
    }
    sync_grab_cursor(&sh.app);
    build_toolbar(sh, h);
    ui::view_menu::build(&mut sh.app, h);
    let first = tabs_for(mode)[0].id;
    show_tab(sh, h, first);
    fit_view(&mut sh.app);
    // The toolbar was built before the fit, so its slider is showing whatever
    // the camera was on in the mode just left.
    sync_zoom(&sh.app);
}

/// Pulls the camera back to fill a stage that has just grown, and no further.
/// Somebody who zoomed into one corner of a map keeps their place and simply
/// sees more around it; somebody looking at the whole thing stops looking at it
/// through a small window.
pub fn fill_view(app: &mut App) {
    let want = match app.mode {
        Mode::Sprites => match app.sheet_dims() {
            Some((w, h)) => app.viewport.fit_flat_zoom(w, h),
            None => return,
        },
        _ => app.viewport.fit_zoom(&active_world_size(app)),
    };
    if app.viewport.zoom < want {
        fit_view(app);
    }
}

/// Frames whatever the mode is showing. A sheet is a handful of pixels across
/// and wants a whole number zoom; a world is thousands and wants to fill the
/// stage.
pub fn fit_view(app: &mut App) {
    if app.mode == Mode::Sprites {
        if let Some((w, h)) = app.sheet_dims() {
            app.viewport.fit_flat(w, h);
        }
        return;
    }
    let world = active_world_size(app);
    app.viewport.fit(&world);
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
            .attr("data-tab", t.id)
            .text(t.label)
            .on("click", Scope::Toolbar, move |_| {
                ui::restart_bar::leaving(
                    &h2,
                    Rc::new(move |h: &Handle| {
                        let mut sh = h.borrow_mut();
                        let sh = &mut *sh;
                        show_tab(sh, h, tid);
                    }),
                );
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
    // The panel was just rebuilt, so the stars in it have to be put back.
    ui::restart_bar::sync(&sh.app);
}

// ---- stage toolbar -------------------------------------------------------

/// The speed slider runs on a log scale. The settings anybody watches are
/// bunched at the slow end, and a linear slider up to two hundred would spend
/// nearly all of its travel between speeds nothing can be told apart at.
pub const SPEED_MIN: f64 = 0.25;
pub const SPEED_MAX: f64 = 200.0;
const SPEED_STEPS: f64 = 400.0;

fn speed_from_slider(pos: f64) -> f64 {
    let t = clamp(pos / SPEED_STEPS, 0.0, 1.0);
    let v = SPEED_MIN * (SPEED_MAX / SPEED_MIN).powf(t);
    // Snapped to something that reads as a setting rather than as a reading.
    if v < 1.0 {
        (v * 100.0).round() / 100.0
    } else if v < 10.0 {
        (v * 10.0).round() / 10.0
    } else {
        v.round()
    }
}

fn slider_from_speed(speed: f64) -> f64 {
    let v = clamp(speed, SPEED_MIN, SPEED_MAX);
    ((v / SPEED_MIN).ln() / (SPEED_MAX / SPEED_MIN).ln() * SPEED_STEPS).round()
}

fn speed_text(v: f64) -> String {
    if v < 10.0 {
        format!("{v}x")
    } else {
        format!("{}x", v.round())
    }
}

fn build_toolbar(sh: &mut Shell, h: &Handle) {
    let toolbar = match by_id("stage-toolbar") {
        Some(n) => n,
        None => return,
    };
    clear(&toolbar);

    if sh.app.mode == Mode::Sprites {
        let controls = sprite_toolbar(sh, h);
        let _ = toolbar.append_child(&el("div").class("toolbar-row").children(controls).get());
        return;
    }

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
    let _ = speed.set_attribute("min", "0");
    let _ = speed.set_attribute("max", &format!("{SPEED_STEPS}"));
    let _ = speed.set_attribute("step", "1");
    speed.set_value(&format!("{}", slider_from_speed(cfg.speed)));
    let speed_label = el("span").class("readout").text(&speed_text(cfg.speed)).get();
    {
        let h2 = h.clone();
        let label = speed_label.clone();
        on(speed.unchecked_ref(), "input", Scope::Toolbar, move |e| {
            let pos: f64 = ui::value_of(&e).parse().unwrap_or(0.0);
            let v = speed_from_slider(pos);
            let mut sh = h2.borrow_mut();
            sh.app.set_speed(v);
            label.set_text_content(Some(&speed_text(v)));
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

    controls.push(zoom_control(sh, h));

    controls.push(ui::button("Fit", Scope::Toolbar, {
        let h2 = h.clone();
        move || {
            let mut sh = h2.borrow_mut();
            fit_view(&mut sh.app);
            sync_zoom(&sh.app);
        }
    }));

    // What the stage draws over the map is in the view menu in the side panel;
    // what the stage does with a press belongs here, next to the map.
    if sh.app.mode == Mode::Settlement {
        // Picking settlers up is a way of using the stage rather than a
        // setting of the project, so it is not saved with one.
        let h2 = h.clone();
        let move_people = ui::toggle_button(
            "Move people",
            sh.app.ui.move_people,
            Scope::Toolbar,
            move |on| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.move_people = on;
                sync_grab_cursor(&sh.app);
                let note = if on {
                    "drag a settler to put them somewhere else - ctrl or middle drag moves the map"
                } else {
                    "the stage moves the map again"
                };
                sh.app.set_note(note);
            },
        );
        let _ = move_people.set_attribute("id", "move-people");
        let _ = move_people.set_attribute(
            "title",
            "drag settlers about; ctrl or the middle button still moves the map",
        );
        controls.push(move_people);
    }

    let _ = toolbar.append_child(&el("div").class("toolbar-row").children(controls).get());
}

/// The sprite editor's own stage controls: which sheet, playing it back,
/// stepping through it, and the camera. No simulation, so no speed.
fn sprite_toolbar(sh: &mut Shell, h: &Handle) -> Vec<Element> {
    let mut controls: Vec<Element> = Vec::new();

    let options = sh.app.state.art.names();
    if !options.is_empty() {
        let picker = ui::select_bare(&sh.app.ui.selected_sheet, &options, {
            let h2 = h.clone();
            move |v| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.selected_sheet = v;
                sh.app.ui.sheet_layer = 0;
                sh.app.ui.sheet_frame = 0;
                sh.app.ui.playing = false;
                fit_view(&mut sh.app);
                sh.app.rebuild_panel();
            }
        });
        controls.push(
            el("label")
                .class("inline")
                .child(&el("span").text("Sheet").get())
                .child(&picker)
                .get(),
        );
    }

    let play = el("button")
        .class("btn")
        .attr("id", "btn-play")
        .attr("type", "button")
        .text(if sh.app.ui.playing { "Pause" } else { "Play" })
        .get();
    {
        let h2 = h.clone();
        let play_node = play.clone();
        on(play.unchecked_ref(), "click", Scope::Toolbar, move |_| {
            let mut sh = h2.borrow_mut();
            sh.app.ui.playing = !sh.app.ui.playing;
            sh.app.ui.play_time = 0.0;
            play_node.set_text_content(Some(if sh.app.ui.playing { "Pause" } else { "Play" }));
            sh.app.redraw_panel = true;
        });
    }
    controls.push(play);

    for (label, delta) in [("Prev", -1), ("Next", 1)] {
        let h2 = h.clone();
        controls.push(ui::button(label, Scope::Toolbar, move || {
            step_frame(&mut h2.borrow_mut().app, delta);
        }));
    }
    controls.push(el("span").class("readout").attr("id", "frame-readout").get());

    let onion = ui::input_el("checkbox");
    onion.set_checked(sh.app.ui.onion);
    let _ = onion.set_attribute("id", "onion");
    {
        let h2 = h.clone();
        on(onion.unchecked_ref(), "change", Scope::Toolbar, move |e| {
            h2.borrow_mut().app.ui.onion = ui::checked_of(&e);
        });
    }
    controls.push(
        el("label")
            .class("inline")
            .child(&el("span").text("Onion").get())
            .child(onion.unchecked_ref())
            .get(),
    );

    controls.push(zoom_control(sh, h));
    controls.push(ui::button("Fit", Scope::Toolbar, {
        let h2 = h.clone();
        move || {
            let mut sh = h2.borrow_mut();
            fit_view(&mut sh.app);
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
    controls.push(note_hint("left draws, right erases, middle or ctrl drags"));
    controls
}

/// The camera slider, shared by every mode. A sprite is looked at far closer
/// than a map, so the top of the range is set by what is being shown.
fn zoom_control(sh: &Shell, h: &Handle) -> Element {
    let top = if sh.app.mode == Mode::Sprites { ZOOM_MAX_SPRITE } else { ZOOM_MAX_WORLD };
    let zoom = ui::input_el("range");
    let _ = zoom.set_attribute("min", "0.5");
    let _ = zoom.set_attribute("max", &format!("{top}"));
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
    el("label")
        .class("inline")
        .child(&el("span").text("Zoom").get())
        .child(zoom.unchecked_ref())
        .child(&zoom_label)
        .get()
}

fn note_hint(text: &str) -> Element {
    el("span").class("readout hint").text(text).get()
}

fn sync_zoom(app: &App) {
    let top = if app.mode == Mode::Sprites { ZOOM_MAX_SPRITE } else { ZOOM_MAX_WORLD };
    if let Some(node) = by_id("zoom-input") {
        if let Ok(input) = node.dyn_into::<HtmlInputElement>() {
            input.set_value(&format!("{}", clamp(app.viewport.zoom, 0.5, top)));
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

    // Every pointer currently down on the map, by its id. One drags the map,
    // two pinch it, and a finger lifting mid-pinch leaves the other one
    // dragging rather than jumping the view.
    let touches: Rc<RefCell<Vec<(i32, f64, f64)>>> = Rc::new(RefCell::new(Vec::new()));
    // The cell a stroke last reached, when the stage is being drawn on rather
    // than dragged. The sprite editor shares this canvas with the camera, so
    // the stroke is driven here rather than by `paint::attach`.
    let stroke: Rc<RefCell<Option<(i32, i32)>>> = Rc::new(RefCell::new(None));
    // The settler currently in hand, or 0. Held here beside the stroke for the
    // same reason: the stage is shared with the camera, so what a press means
    // is decided in one place.
    let grab: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    // Where two fingers were as of the last move: how far apart, and the point
    // between them. A pinch is read as the change since then rather than since
    // it started, so a finger lifting and landing again carries on from where
    // the gesture is instead of snapping back to where it began.
    let span = Rc::new(Cell::new(0.0f64));
    let middle = Rc::new(Cell::new((0.0f64, 0.0f64)));

    {
        let touches = touches.clone();
        let span = span.clone();
        let middle = middle.clone();
        let canvas2 = canvas.clone();
        let h2 = h.clone();
        let stroke = stroke.clone();
        let grab = grab.clone();
        on(canvas.unchecked_ref(), "pointerdown", Scope::Global, move |e: Event| {
            let pe = e.dyn_ref::<web_sys::PointerEvent>().unwrap();
            let _ = canvas2.set_pointer_capture(pe.pointer_id());
            let mut list = touches.borrow_mut();
            list.retain(|(id, _, _)| *id != pe.pointer_id());
            list.push((pe.pointer_id(), pe.client_x() as f64, pe.client_y() as f64));
            span.set(pinch_span(&list));
            middle.set(pinch_middle(&list));
            // A second pointer is a pinch, whatever the first one was doing.
            if list.len() > 1 {
                end_stroke(&h2, &stroke);
                end_grab(&h2, &grab);
                return;
            }
            let mut sh = h2.borrow_mut();
            if grabs(&sh.app, pe) {
                // Pressing on nobody falls through to dragging the map, so the
                // switch does not cost the camera the whole stage.
                if let Some(id) = start_grab(&mut sh.app, pe.client_x() as f64, pe.client_y() as f64)
                {
                    grab.set(id);
                    set_holding(true);
                    return;
                }
            }
            if !paints(&sh.app, pe) {
                return;
            }
            let cell = SHEET_SURFACE.locate(
                &sh.app,
                &sh.app.viewport.canvas.clone(),
                pe.client_x() as f64,
                pe.client_y() as f64,
            );
            if let Some(cell) = cell {
                if sh.app.ui.tool == Tool::Select {
                    // A press starts a new selection wherever it lands, so
                    // there is no way to end up with a stale one under the
                    // pointer.
                    sh.app.ui.marquee = Some((cell.0, cell.1, cell.0, cell.1));
                    *stroke.borrow_mut() = Some(cell);
                    sh.app.redraw_panel = true;
                    return;
                }
                if sh.app.ui.tool != Tool::Pick {
                    sh.app.record("stroke", false);
                }
                let erase = pe.buttons() & 2 == 2;
                ui::paint::apply(&mut sh.app, &SHEET_SURFACE, cell, erase);
                *stroke.borrow_mut() = Some(cell);
            }
        });
    }
    {
        let touches = touches.clone();
        let span = span.clone();
        let middle = middle.clone();
        let stroke = stroke.clone();
        let grab = grab.clone();
        let h2 = h.clone();
        on(canvas.unchecked_ref(), "pointermove", Scope::Global, move |e: Event| {
            let pe = e.dyn_ref::<web_sys::PointerEvent>().unwrap();
            let (x, y) = (pe.client_x() as f64, pe.client_y() as f64);
            let mut list = touches.borrow_mut();
            let previous = match list.iter_mut().find(|(id, _, _)| *id == pe.pointer_id()) {
                Some(slot) => {
                    let was = (slot.1, slot.2);
                    slot.1 = x;
                    slot.2 = y;
                    was
                }
                None => return,
            };
            let mut sh = h2.borrow_mut();
            if list.len() < 2 && grab.get() != 0 {
                drag_held(&mut sh.app, x, y);
                return;
            }
            if list.len() < 2 && stroke.borrow().is_some() {
                let cell = SHEET_SURFACE.locate(
                    &sh.app,
                    &sh.app.viewport.canvas.clone(),
                    x,
                    y,
                );
                if let Some(cell) = cell {
                    if sh.app.ui.tool == Tool::Select {
                        // The far corner follows the pointer; the near one is
                        // where the press landed and does not move.
                        if let Some(m) = &mut sh.app.ui.marquee {
                            if (m.2, m.3) != cell {
                                m.2 = cell.0;
                                m.3 = cell.1;
                                sh.app.redraw_panel = true;
                            }
                        }
                        return;
                    }
                    let was = *stroke.borrow();
                    if was != Some(cell) {
                        let erase = pe.buttons() & 2 == 2;
                        let freehand = matches!(sh.app.ui.tool, Tool::Pencil | Tool::Eraser);
                        match was {
                            Some(prev) if freehand => {
                                ui::paint::stroke_line(&mut sh.app, &SHEET_SURFACE, prev, cell, erase)
                            }
                            _ => ui::paint::apply(&mut sh.app, &SHEET_SURFACE, cell, erase),
                        }
                        *stroke.borrow_mut() = Some(cell);
                    }
                }
                return;
            }
            if list.len() >= 2 {
                let now = pinch_span(&list);
                let (mx, my) = pinch_middle(&list);
                let before = span.get();
                let (px, py) = middle.get();
                span.set(now);
                middle.set((mx, my));
                if before > 0.0 && now > 0.0 {
                    // Two fingers both move the map and scale it: the point
                    // between them drags, and their separation zooms about it.
                    sh.app.viewport.pan(mx - px, my - py);
                    sh.app.viewport.zoom_at(mx, my, now / before);
                    sync_zoom(&sh.app);
                }
                return;
            }
            sh.app.viewport.pan(x - previous.0, y - previous.1);
        });
    }
    for event in ["pointerup", "pointercancel", "pointerleave"] {
        let touches = touches.clone();
        let span = span.clone();
        let middle = middle.clone();
        let stroke = stroke.clone();
        let grab = grab.clone();
        let h2 = h.clone();
        on(canvas.unchecked_ref(), event, Scope::Global, move |e: Event| {
            let pe = e.dyn_ref::<web_sys::PointerEvent>().unwrap();
            {
                let mut list = touches.borrow_mut();
                list.retain(|(id, _, _)| *id != pe.pointer_id());
                span.set(pinch_span(&list));
                middle.set(pinch_middle(&list));
            }
            end_stroke(&h2, &stroke);
            end_grab(&h2, &grab);
        });
    }
    {
        // The right button erases, so the menu it would open would land in the
        // middle of the stroke.
        let h2 = h.clone();
        on(canvas.unchecked_ref(), "contextmenu", Scope::Global, move |e: Event| {
            if h2.borrow().app.mode == Mode::Sprites {
                e.prevent_default();
            }
        });
    }
}

/// How far from a press a settler will still be picked up, in cells. A settler
/// is about a cell across, so this is wide enough to catch one aimed at and
/// narrow enough to leave the one beside them alone.
const GRAB_REACH: f64 = 1.6;

/// Whether this press picks a settler up rather than dragging the map. Only in
/// the settlement, only with the switch on, and only for a plain press: the
/// middle button and a held control key drag the map, the same way they do in
/// the sprite editor.
fn grabs(app: &App, pe: &web_sys::PointerEvent) -> bool {
    app.mode == Mode::Settlement
        && app.ui.move_people
        && app.settlement.is_some()
        && !pe.ctrl_key()
        && pe.button() != 1
        && pe.buttons() & 4 == 0
}

/// Picks up whoever is under the pointer, and says who that was.
fn start_grab(app: &mut App, client_x: f64, client_y: f64) -> Option<u32> {
    let world = app.settlement.as_ref()?.world().clone();
    let (gx, gy) = app.viewport.ground_at(client_x, client_y, &world)?;
    let sim = app.settlement.as_mut()?;
    let id = sim.person_near(gx, gy, GRAB_REACH)?;
    if !sim.hold_person(id) {
        return None;
    }
    let name = sim.people.get(id).map(|p| p.name.clone()).unwrap_or_default();
    app.set_note(&format!("holding {name}"));
    Some(id)
}

/// Carries the held settler along with the pointer.
fn drag_held(app: &mut App, client_x: f64, client_y: f64) {
    let world = match app.settlement.as_ref() {
        Some(sim) => sim.world().clone(),
        None => return,
    };
    let at = app.viewport.ground_at(client_x, client_y, &world);
    if let (Some((gx, gy)), Some(sim)) = (at, app.settlement.as_mut()) {
        sim.move_held(gx, gy);
    }
}

/// Puts down whoever is in hand, wherever the pointer left them.
fn end_grab(h: &Handle, grab: &Rc<Cell<u32>>) {
    if grab.get() == 0 {
        return;
    }
    let id = grab.replace(0);
    set_holding(false);
    let mut sh = h.borrow_mut();
    let landed = match sh.app.settlement.as_mut() {
        Some(sim) => sim.drop_held().map(|_| {
            sim.people.get(id).map(|p| p.name.clone()).unwrap_or_default()
        }),
        None => None,
    };
    if let Some(name) = landed {
        sh.app.set_note(&format!("put {name} down"));
    }
}

/// What the stage says a press will do: an open hand where settlers can be
/// picked up, a closed one while one is in hand.
fn sync_grab_cursor(app: &App) {
    let body = match document().body() {
        Some(b) => b,
        None => return,
    };
    let list = body.class_list();
    let on = app.mode == Mode::Settlement && app.ui.move_people;
    let _ = if on { list.add_1("moving-people") } else { list.remove_1("moving-people") };
    if !on {
        set_holding(false);
    }
}

fn set_holding(holding: bool) {
    let body = match document().body() {
        Some(b) => b,
        None => return,
    };
    let list = body.class_list();
    let _ = if holding { list.add_1("holding") } else { list.remove_1("holding") };
}

/// The sheet, as something to draw on. Held as a constant because the stage
/// shares one surface for as long as the mode is open, unlike a panel editor
/// which is rebuilt with its canvas.
const SHEET_SURFACE: ui::art_panel::SheetSurface = ui::art_panel::SheetSurface;

/// Whether this pointer press is a stroke rather than a drag. Only in the
/// sprite editor, and only for a plain press: the middle button and a held
/// control key are how the stage is dragged there, since the left one is busy.
fn paints(app: &App, pe: &web_sys::PointerEvent) -> bool {
    app.mode == Mode::Sprites
        && app.sheet_dims().is_some()
        && !pe.ctrl_key()
        && pe.button() != 1
        && pe.buttons() & 4 == 0
}

fn end_stroke(h: &Handle, stroke: &Rc<RefCell<Option<(i32, i32)>>>) {
    if stroke.borrow().is_none() {
        return;
    }
    *stroke.borrow_mut() = None;
    let mut sh = h.borrow_mut();
    if sh.app.ui.tool == Tool::Select {
        // The panel says what the selection covers, so it is rebuilt once the
        // drag is over rather than at every value the pointer passes through.
        sh.app.rebuild_panel();
        return;
    }
    SHEET_SURFACE.commit(&mut sh.app);
    sh.app.redraw_panel = true;
}

/// Distance between the first two pointers down, or nothing if there are not
/// two of them, which is what tells a pinch from a drag.
fn pinch_span(list: &[(i32, f64, f64)]) -> f64 {
    if list.len() < 2 {
        return 0.0;
    }
    let (dx, dy) = (list[0].1 - list[1].1, list[0].2 - list[1].2);
    (dx * dx + dy * dy).sqrt()
}

/// The point between the first two pointers down. With fewer than two there is
/// no pinch in progress and nothing for it to be measured against.
fn pinch_middle(list: &[(i32, f64, f64)]) -> (f64, f64) {
    if list.len() < 2 {
        return (0.0, 0.0);
    }
    ((list[0].1 + list[1].1) / 2.0, (list[0].2 + list[1].2) / 2.0)
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
        if ke.key() == "/" && !ke.ctrl_key() && !ke.meta_key() && !ke.alt_key() {
            // Search is the one thing worth a bare key: it is how somebody who
            // does not know where a setting lives gets to it.
            e.prevent_default();
            ui::find_box::focus();
            return;
        }
        let mut sh = h2.borrow_mut();
        if ke.code() == "Escape" && stage_only() && document().fullscreen_element().is_none() {
            // With no fullscreen to leave, escape does nothing on its own, and
            // the page would be showing the world with no obvious way back.
            set_stage_only(&mut sh.app, false);
            return;
        }
        if ke.ctrl_key() || ke.meta_key() {
            match ke.key().to_ascii_lowercase().as_str() {
                "z" if ke.shift_key() => {
                    e.prevent_default();
                    sh.app.redo();
                }
                "z" => {
                    e.prevent_default();
                    sh.app.undo();
                }
                "y" => {
                    e.prevent_default();
                    sh.app.redo();
                }
                _ => {}
            }
            return;
        }
        if ke.code() == "Escape" && sh.app.ui.marquee.is_some() {
            sh.app.ui.marquee = None;
            sh.app.rebuild_panel();
            return;
        }
        if sh.app.mode == Mode::Sprites
            && sprite_key(&mut sh, &ke.key().to_ascii_lowercase())
        {
            e.prevent_default();
            return;
        }
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
                    // A step is a tick of the simulation, or a frame of the
                    // sheet where there is no simulation to tick.
                    if sh.app.mode == Mode::Sprites {
                        step_frame(&mut sh.app, 1);
                    } else {
                        let dt = 1.0 / sh.app.active_tick_hz();
                        step_active(&mut sh.app, dt);
                    }
                }
                "f" => {
                    fit_view(&mut sh.app);
                    sync_zoom(&sh.app);
                }
                "m" => {
                    // Round the row of modes, in the order they are shown.
                    let at = MODES.iter().position(|(m, _)| *m == sh.app.mode).unwrap_or(0);
                    let next = MODES[(at + 1) % MODES.len()].0;
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

/// The controls that belong to the window rather than to the project: folding
/// the menu away, and how large everything is drawn.
fn bind_view_actions(h: &Handle) {
    let prefs = ui::prefs::Prefs::load();
    prefs.apply();
    ui::view_menu::restore_fold(prefs.view_open);
    bind_fullscreen(h);

    if let Some(node) = by_id("btn-panel") {
        node.set_text_content(Some(collapse_label(prefs.collapsed)));
        let label = node.clone();
        let h2 = h.clone();
        on(node.unchecked_ref(), "click", Scope::Global, move |_| {
            let mut prefs = ui::prefs::Prefs::load();
            prefs.collapsed = !prefs.collapsed;
            let mut sh = h2.borrow_mut();
            prefs.apply();
            // Folding the menu away hands its width to the stage; the camera
            // keeps the middle of the view in the middle across the change.
            sh.app.viewport.resize();
            prefs.save();
            label.set_text_content(Some(collapse_label(prefs.collapsed)));
        });
    }

    if let Some(node) = by_id("ui-scale") {
        if let Ok(input) = node.dyn_into::<HtmlInputElement>() {
            input.set_value(&format!("{}", prefs.scale));
            on(input.unchecked_ref(), "input", Scope::Global, move |e| {
                let v: f64 = ui::value_of(&e).parse().unwrap_or(1.0);
                let mut prefs = ui::prefs::Prefs::load();
                prefs.scale = clamp(v, ui::prefs::SCALE_MIN, ui::prefs::SCALE_MAX);
                prefs.apply();
                prefs.save();
            });
        }
    }
}

/// Whether the page is showing the world and nothing else.
fn stage_only() -> bool {
    document()
        .body()
        .map(|b| b.class_list().contains("stage-only"))
        .unwrap_or(false)
}

/// Folds every piece of chrome away, or brings it back. The camera is told
/// afterwards because the stage has just changed size by most of the window.
fn set_stage_only(app: &mut App, on: bool) {
    if let Some(body) = document().body() {
        let list = body.class_list();
        let _ = if on { list.add_1("stage-only") } else { list.remove_1("stage-only") };
    }
    app.viewport.resize();
    fill_view(app);
    sync_zoom(app);
    if let Some(node) = by_id("btn-full") {
        node.set_text_content(Some(if on { "Leave fullscreen" } else { "Fullscreen" }));
    }
}

/// The button, the escape hatch on the stage, and the browser's own way out.
///
/// Asking for the screen and folding the chrome away are two separate things,
/// and either can happen without the other: a browser can refuse the request,
/// and escape leaves the screen without telling the page anything except
/// through `fullscreenchange`. Keeping them together means listening for that
/// and following it.
fn bind_fullscreen(h: &Handle) {
    for id in ["btn-full", "btn-leave-full"] {
        let btn = match by_id(id) {
            Some(b) => b,
            None => continue,
        };
        let h2 = h.clone();
        on(btn.unchecked_ref(), "click", Scope::Global, move |_| {
            let want = !stage_only();
            set_stage_only(&mut h2.borrow_mut().app, want);
            let doc = document();
            if want {
                if let Some(root) = doc.document_element() {
                    let _ = root.request_fullscreen();
                }
            } else if doc.fullscreen_element().is_some() {
                doc.exit_fullscreen();
            }
        });
    }

    bind_idle_fade();

    let h2 = h.clone();
    on(document().unchecked_ref(), "fullscreenchange", Scope::Global, move |_| {
        let entered = document().fullscreen_element().is_some();
        if !stage_only() {
            // Fullscreen the page did not ask for, from the browser's own key.
            return;
        }
        let mut sh = h2.borrow_mut();
        if entered {
            // The window only reaches its full size after the request settles,
            // so this is the first moment the camera can be framed for it.
            sh.app.viewport.resize();
            fill_view(&mut sh.app);
            sync_zoom(&sh.app);
        } else {
            // Leaving by escape or by the window manager says nothing else, so
            // the chrome comes back from here.
            set_stage_only(&mut sh.app, false);
        }
    });
}

/// How long the pointer has to be still before the one piece of chrome left in
/// fullscreen gets out of the way as well.
const IDLE_MS: i32 = 2_500;

/// Fades the escape hatch out once nothing has moved for a while, and brings it
/// back on the first sign of life. Only the class is managed here; what it does
/// is the stylesheet's business, and it does nothing at all outside fullscreen.
fn bind_idle_fade() {
    /// The callback the timer fires, held for as long as the page is up.
    type Arm = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;
    let timer: Rc<Cell<i32>> = Rc::new(Cell::new(0));
    let arm: Arm = Rc::new(RefCell::new(None));
    {
        let arm2 = arm.clone();
        *arm.borrow_mut() = Some(Closure::wrap(Box::new(move || {
            let _ = &arm2;
            if let Some(body) = document().body() {
                let _ = body.class_list().add_1("idle");
            }
        }) as Box<dyn FnMut()>));
    }

    let wake = move |_: Event| {
        let body = match document().body() {
            Some(b) => b,
            None => return,
        };
        let _ = body.class_list().remove_1("idle");
        if timer.get() != 0 {
            window().clear_timeout_with_handle(timer.get());
            timer.set(0);
        }
        if !body.class_list().contains("stage-only") {
            return;
        }
        if let Some(cb) = arm.borrow().as_ref() {
            if let Ok(id) = window().set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                IDLE_MS,
            ) {
                timer.set(id);
            }
        }
    };
    // Anything a person does counts, including leaving fullscreen, which is why
    // this listens on the window rather than on the stage.
    for event in ["pointermove", "pointerdown", "keydown", "wheel", "fullscreenchange"] {
        on(window().unchecked_ref(), event, Scope::Global, wake.clone());
    }
}

fn collapse_label(collapsed: bool) -> &'static str {
    if collapsed {
        "Show menu"
    } else {
        "Hide menu"
    }
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

    for (id, forward) in [("btn-undo", false), ("btn-redo", true)] {
        let btn = match by_id(id) {
            Some(b) => b,
            None => continue,
        };
        let h2 = h.clone();
        on(btn.unchecked_ref(), "click", Scope::Global, move |_| {
            let mut sh = h2.borrow_mut();
            if forward {
                sh.app.redo();
            } else {
                sh.app.undo();
            }
        });
    }

    if let Some(btn) = by_id("btn-reset") {
        on(btn.unchecked_ref(), "click", Scope::Global, move |_| {
            let asked = window()
                .confirm_with_message(
                    "Clear everything this page has saved in the browser and start again? \
                     The project, the window settings and any cached files all go, and this \
                     cannot be undone. Kept sheets are left alone; they are deleted from the \
                     sprite editor's Sheet tab.",
                )
                .unwrap_or(false);
            if asked {
                ui::reset::everything();
            }
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
    // The steps in the history are snapshots of a project that is no longer
    // open, and putting one back would graft it onto this one.
    app.history.clear();
    app.ui.selected_sampler = app
        .state
        .materials
        .samplers
        .first()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    app.ui.selected_species = app.state.species.first().map(|s| s.id.clone()).unwrap_or_default();
    app.ui.shade_preview_sampler = app.ui.selected_sampler.clone();
    app.ui.selected_sheet = app.state.art.sheets.first().map(|s| s.id.clone()).unwrap_or_default();
    app.ui.sheet_layer = 0;
    app.ui.sheet_frame = 0;
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
    ui::download(&url, "grow-project.json");
    let _ = web_sys::Url::revoke_object_url(&url);
}

// ---- frame loop ----------------------------------------------------------

fn step_active(app: &mut App, dt: f64) {
    match app.mode {
        Mode::Sprites => {}
        Mode::Lab => {
            let App { sim, state, .. } = app;
            sim.step(state, dt, None);
        }
        Mode::Settlement => {
            if let Some(civ) = &mut app.settlement {
                civ.step(&app.state, dt);
            }
            check_extinction(app);
        }
    }
}

/// Starts the settlement over once it has been empty long enough, if that is
/// what it was told to do. The wait runs on settlement time, so a paused world
/// stays where it is and a fast one gets there sooner.
fn check_extinction(app: &mut App) {
    let start = &app.state.civ.start;
    if !start.restart_when_gone || app.pending_civ_reset {
        return;
    }
    let empty = match app.settlement.as_ref().and_then(|civ| civ.extinct_for()) {
        Some(t) => t,
        None => return,
    };
    if empty < start.restart_after.max(0.0) {
        return;
    }
    app.civ_restart();
    app.set_note("nobody left; starting over");
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

    if sh.app.mode == Mode::Sprites {
        if sh.app.ui.playing {
            sh.app.ui.play_time += dt_real;
        }
        if let Some(node) = by_id("frame-readout") {
            let sheet = sh.app.state.art.find(&sh.app.ui.selected_sheet);
            let count = sheet.map(|s| s.frame_count()).unwrap_or(1);
            node.set_text_content(Some(&format!("{}/{count}", active_frame(&sh.app) + 1)));
        }
    }

    let cfg = sh.app.sim_settings();
    let civ_ready = sh.app.settlement.as_ref().is_some_and(|c| c.ready);
    if cfg.running && (sh.app.mode == Mode::Lab || (sh.app.mode == Mode::Settlement && civ_ready)) {
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
    if ts - sh.app.status_at >= STATUS_INTERVAL_MS {
        sh.app.status_at = ts;
        update_status(&sh.app);
    }

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
        Mode::Sprites => draw_sheet(app),
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
            civ.px_step = app.viewport.sample_step();
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
            app.viewport.draw_colony_labels(&civ, &app.state);
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

/// The sheet on the stage: a checker, the frame before this one behind it when
/// onion skin is on, and the frame itself. Built as a flat buffer and pushed
/// through the same camera as a world, so zoom, pan and pinch all work the way
/// they do everywhere else.
fn draw_sheet(app: &mut App) {
    let sheet = match app.state.art.find(&app.ui.selected_sheet) {
        Some(s) => s,
        None => return,
    };
    let (w, h) = (sheet.w.max(1), sheet.h.max(1));
    let frame = active_frame(app);
    let flat = sheet.flatten(frame);
    let ghost = if app.ui.onion && sheet.frame_count() > 1 {
        let before = (frame + sheet.frame_count() - 1) % sheet.frame_count();
        Some(sheet.flatten(before))
    } else {
        None
    };

    let mut buf = vec![0u32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            // The checker is drawn into the buffer rather than under it, so it
            // scales with the art and reads as one square per pixel.
            let mut c = if (x + y) % 2 == 0 { CHECKER_LIGHT } else { CHECKER_DARK };
            if let Some(ghost) = &ghost {
                if ghost[i] != EMPTY_COLOR {
                    c = crate::util::mix_packed(c, ghost[i], 0.3);
                }
            }
            if flat[i] != EMPTY_COLOR {
                c = flat[i];
            }
            buf[i] = c;
        }
    }
    // The selection outline goes over the art rather than under it, and is
    // dashed, so it reads as a selection rather than as something drawn.
    if let Some((x0, y0, x1, y1)) = app.ui.marquee_rect(w, h) {
        for x in x0..=x1 {
            for y in [y0, y1] {
                if (x + y) % 2 == 0 {
                    buf[(y * w + x) as usize] = MARQUEE_COLOR;
                }
            }
        }
        for y in y0..=y1 {
            for x in [x0, x1] {
                if (x + y) % 2 == 0 {
                    buf[(y * w + x) as usize] = MARQUEE_COLOR;
                }
            }
        }
    }
    app.viewport.present_flat(w, h, &buf);
    if app.viewport.show_grid {
        app.viewport.draw_pixel_grid(w, h);
    }
    app.viewport.finish();
}

/// Which frame the stage is showing: the one being drawn, or wherever the
/// playback has got to.
fn active_frame(app: &App) -> i32 {
    let sheet = match app.state.art.find(&app.ui.selected_sheet) {
        Some(s) => s,
        None => return 0,
    };
    if !app.ui.playing {
        return app.ui.sheet_frame.clamp(0, sheet.frame_count() - 1);
    }
    let n = sheet.frame_count();
    if n <= 1 || sheet.fps <= 0.0 {
        return 0;
    }
    ((app.ui.play_time * sheet.fps).floor() as i64).rem_euclid(n as i64) as i32
}

/// Moves to another frame of the sheet, stopping playback: stepping and
/// playing at once is two things trying to say which frame is showing.
/// What the sprite editor answers to from the keyboard. Listed in the Draw
/// panel wherever there is a keyboard to use them from, because a shortcut
/// nobody can find is not a shortcut.
pub const SPRITE_KEYS: [(&str, &str); 12] = [
    ("B", "Pencil"),
    ("E", "Eraser"),
    ("G", "Fill"),
    ("P", "Pick"),
    ("M", "Marquee"),
    ("X", "Mirror X"),
    ("O", "Onion skin"),
    ("Space", "Play or pause"),
    (", .", "Frame back or on"),
    ("[ ]", "Layer down or up"),
    ("F", "Fit the sheet to the stage"),
    ("Ctrl Z", "Undo; with shift, redo"),
];

fn step_layer(app: &mut App, delta: i32) {
    let layers = match app.state.art.find(&app.ui.selected_sheet) {
        Some(s) => s.layers.len() as i32,
        None => return,
    };
    if layers < 1 {
        return;
    }
    app.ui.sheet_layer = (app.ui.sheet_layer as i32 + delta).rem_euclid(layers) as usize;
    app.rebuild_panel();
}

/// The tool keys and the two switches that live in the toolbar rather than in
/// the panel. Returns whether the key was one of these.
fn sprite_key(sh: &mut Shell, key: &str) -> bool {
    match key {
        "b" => sh.app.ui.tool = Tool::Pencil,
        "e" => sh.app.ui.tool = Tool::Eraser,
        "g" => sh.app.ui.tool = Tool::Fill,
        "p" => sh.app.ui.tool = Tool::Pick,
        "m" => sh.app.ui.tool = Tool::Select,
        "x" => sh.app.ui.mirror_x = !sh.app.ui.mirror_x,
        "o" => {
            sh.app.ui.onion = !sh.app.ui.onion;
            // The onion switch is in the stage toolbar, which a panel rebuild
            // does not touch, so it is set to match by hand.
            if let Some(node) = by_id("onion") {
                if let Ok(input) = node.dyn_into::<HtmlInputElement>() {
                    input.set_checked(sh.app.ui.onion);
                }
            }
        }
        "," => {
            step_frame(&mut sh.app, -1);
            return true;
        }
        "[" => {
            step_layer(&mut sh.app, -1);
            return true;
        }
        "]" => {
            step_layer(&mut sh.app, 1);
            return true;
        }
        _ => return false,
    }
    sh.app.rebuild_panel();
    true
}

fn step_frame(app: &mut App, delta: i32) {
    let frames = match app.state.art.find(&app.ui.selected_sheet) {
        Some(s) => s.frame_count(),
        None => return,
    };
    app.ui.playing = false;
    app.ui.sheet_frame = (app.ui.sheet_frame + delta).rem_euclid(frames);
    app.rebuild_panel();
}

fn sheet_status(app: &App) -> String {
    let sheet = match app.state.art.find(&app.ui.selected_sheet) {
        Some(s) => s,
        None => return "no sheet".to_string(),
    };
    let layer = sheet
        .layers
        .get(app.ui.sheet_layer)
        .map(|l| l.name.clone())
        .unwrap_or_default();
    [
        sheet.name.clone(),
        format!("{}x{}", sheet.w, sheet.h),
        format!("frame {}/{}", active_frame(app) + 1, sheet.frame_count()),
        format!("layer {} of {}", app.ui.sheet_layer + 1, sheet.layers.len()),
        layer,
        format!("{} fps", sheet.fps),
        format!("{:.0}x zoom", app.viewport.zoom),
        format!("{:.0} fps", app.fps),
    ]
    .iter()
    .filter(|part| !part.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join("   ")
}

fn update_status(app: &App) {
    let status = match by_id("statusbar") {
        Some(n) => n,
        None => return,
    };
    let text = match app.mode {
        Mode::Sprites => sheet_status(app),
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
