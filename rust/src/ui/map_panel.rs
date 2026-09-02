//! The map editor: a page of the sprite editor where the settlement's own map
//! is drawn by hand.
//!
//! It borrows the pixel editor whole. The stage is a grid one pixel per map
//! cell, the drawing tools are the ones already in the toolbar - pencil,
//! eraser, fill, pick, line, mirror - and the colors are a legend of what land
//! is rather than a wheel, so picking a color is choosing what a cell should
//! be. What differs from the sheet editor is where the paint lands: there is
//! no draft to apply, and a stroke changes the running map under the pointer.
//!
//! Three things are painted on and they are not the same kind of thing at all.
//! The ground is the map: water, rock, a rock face, grass, sand. A zone says
//! what may take root and is drawn nowhere but here, which is what makes it a
//! way of saying where the wood should be rather than a mark on the world. Sky
//! is neither: it is a mark on this page alone, held for as long as the page
//! is open, and all it does is say where to read a sky out of the picture.
//!
//! A picture can be laid under the map to trace, or read straight in as the
//! map. What decides between them is one number - how many of the picture's
//! pixels go to one cell - which is guessed when the picture arrives.

use wasm_bindgen::JsCast;
use web_sys::{DragEvent, Element, Event, HtmlCanvasElement};

use crate::app::{App, Handle, Panel};
use crate::civ::map_brush::{Brush, BRUSHES};
use crate::civ::terrain::Cell;
use crate::ui::paint::Surface;
use crate::ui::{
    app_button, append, btn_row, button, count_field, danger_button, el, input_el, note, on,
    section, stat, Scope, Tap,
};
use crate::util::EMPTY_COLOR;
use crate::world::Zone;

/// How much of the map's own color is laid over the picture being traced.
/// There is no slider for it: with nothing underneath the map is drawn as
/// itself, and with a picture underneath it is drawn far enough back to read
/// the coastline through it.
const OVER_PICTURE: f64 = 0.55;

/// The smallest map a picture is allowed to make. There is no ceiling on the
/// size - a drawing is worth however many cells it was drawn with - but there
/// is a floor, because a town cannot be founded on a map of nine cells and a
/// picture dropped by mistake should not be the thing that finds that out.
const MIN_COLS: i32 = 16;
const MIN_ROWS: i32 = 8;

/// How many strokes can be put back. A stroke holds a cell for every cell it
/// touched, and a fill over a large map is every cell there is, so this is
/// deliberately short.
const STEPS_KEPT: usize = 24;

/// One cell as it was before a stroke touched it.
#[derive(Clone, Copy)]
pub struct Was {
    at: usize,
    kind: u8,
    zone: u8,
    sky: u8,
}

/// One stroke, in the order it was painted.
pub type Step = Vec<Was>;

/// Everything the map editor holds that the map itself does not.
///
/// None of it is saved. A photograph is megabytes and the map it was traced
/// into is what is worth keeping; the sky marks say where to read that
/// photograph and mean nothing without it; and the strokes are a way back out
/// of the last few minutes rather than a history of the town.
#[derive(Default)]
pub struct MapTools {
    pub image: Option<(i32, i32, Vec<u32>)>,
    pub name: String,
    /// Picture pixels to one map cell. Zero means nobody has said, which is
    /// read as one.
    pub px: i32,
    /// Cells marked sky, one byte each, at the size of the map they were
    /// marked on.
    pub sky: Vec<u8>,
    sky_dims: (i32, i32),
    steps: Vec<Step>,
    redone: Vec<Step>,
    open: Option<Step>,
    /// A stroke has changed the ground, so the coarse plant index owes a
    /// rebuild when the pointer lifts.
    ground_dirty: bool,
}

impl MapTools {
    /// The sky marks, grown to the map they are being drawn on. A map that has
    /// changed size loses them, because a mark is a cell and those cells are
    /// not the same cells.
    pub fn ensure(&mut self, cols: i32, rows: i32) {
        if self.sky_dims != (cols, rows) || self.sky.len() != (cols * rows).max(0) as usize {
            self.sky = vec![0; (cols.max(0) * rows.max(0)) as usize];
            self.sky_dims = (cols, rows);
            self.steps.clear();
            self.redone.clear();
        }
    }

    /// How many picture pixels go to a cell, never less than one.
    pub fn scale(&self) -> i32 {
        self.px.max(1)
    }

    /// The map the picture makes at that scale, in cells.
    pub fn picture_cells(&self) -> Option<(i32, i32)> {
        let (w, h, _) = self.image.as_ref()?;
        let n = self.scale();
        Some(((w / n).max(MIN_COLS), (h / n).max(MIN_ROWS)))
    }

    pub fn marked_sky(&self) -> usize {
        self.sky.iter().filter(|&&v| v != 0).count()
    }
}

/// Which of the three things a press paints on. The brush decides: it is one
/// list of answers about a cell, and each answer belongs to one layer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Ground,
    Growth,
    Sky,
}

fn layer_of(app: &App) -> Layer {
    match Brush::from_color(app.ui.brush_color) {
        Brush::Sky => Layer::Sky,
        b if b.ground().is_some() => Layer::Ground,
        // The eraser paints nothing rather than a color, so what it takes off
        // is decided by the brush that is still selected. Everything that is
        // not ground and not sky is a zone, the eraser's own color included.
        _ => Layer::Growth,
    }
}

/// The map as a drawing surface. Colors in, cells out: the legend is the whole
/// translation, and a color the legend does not know is read as clear, which
/// is what makes the eraser and a stray color off the wheel both mean "take
/// the zone off this cell".
pub struct MapSurface;

impl MapSurface {
    fn size(app: &App) -> Option<(i32, i32)> {
        let world = app.settlement.as_ref()?.world();
        Some((world.cols, world.rows))
    }
}

impl Surface for MapSurface {
    fn dims(&self, app: &App) -> Option<(i32, i32)> {
        MapSurface::size(app)
    }

    fn get(&self, app: &App, x: i32, y: i32) -> u32 {
        let sim = match app.settlement.as_ref() {
            Some(sim) => sim,
            None => return EMPTY_COLOR,
        };
        let (cols, rows) = (sim.world().cols, sim.world().rows);
        if x < 0 || y < 0 || x >= cols || y >= rows {
            return EMPTY_COLOR;
        }
        match layer_of(app) {
            Layer::Sky => match app.ui.map_edit.sky.get((y * cols + x) as usize) {
                Some(&1) => Brush::Sky.color(),
                _ => EMPTY_COLOR,
            },
            Layer::Growth => match sim.terrain.zone_at(x, y) {
                Zone::Any => EMPTY_COLOR,
                zone => Brush::of_zone(zone).color(),
            },
            Layer::Ground => Brush::of_ground(sim.terrain.type_at(x, y)).color(),
        }
    }

    /// What is on show under the pointer, which is what the pick tool takes:
    /// the sky mark, then the zone, then the ground.
    fn pick(&self, app: &App, x: i32, y: i32) -> u32 {
        let sim = match app.settlement.as_ref() {
            Some(sim) => sim,
            None => return EMPTY_COLOR,
        };
        let (cols, rows) = (sim.world().cols, sim.world().rows);
        if x < 0 || y < 0 || x >= cols || y >= rows {
            return EMPTY_COLOR;
        }
        if app.ui.map_edit.sky.get((y * cols + x) as usize) == Some(&1) {
            return Brush::Sky.color();
        }
        match sim.terrain.zone_at(x, y) {
            Zone::Any => Brush::of_ground(sim.terrain.type_at(x, y)).color(),
            zone => Brush::of_zone(zone).color(),
        }
    }

    fn set(&self, app: &mut App, x: i32, y: i32, v: u32) {
        let (cols, rows) = match MapSurface::size(app) {
            Some(d) => d,
            None => return,
        };
        if x < 0 || y < 0 || x >= cols || y >= rows {
            return;
        }
        let at = (y * cols + x) as usize;
        app.ui.map_edit.ensure(cols, rows);
        let was = held(app, at);
        let brush = if v == EMPTY_COLOR { Brush::Clear } else { Brush::from_color(v) };
        match layer_of(app) {
            Layer::Sky => {
                let mark = u8::from(brush == Brush::Sky);
                if let Some(slot) = app.ui.map_edit.sky.get_mut(at) {
                    *slot = mark;
                }
            }
            Layer::Growth => {
                let zone = brush.zone().unwrap_or(Zone::Any);
                if let Some(sim) = app.settlement.as_mut() {
                    sim.terrain.set_zone(x, y, zone);
                }
            }
            // Ground is the one layer with nothing to erase: every cell is
            // some kind of ground, so a press with nothing in it says nothing.
            Layer::Ground => {
                if let Some(kind) = brush.ground() {
                    if app.settlement.as_mut().is_some_and(|sim| sim.paint_cell(x, y, kind)) {
                        app.ui.map_edit.ground_dirty = true;
                    }
                }
            }
        }
        let now = held(app, at);
        if now.kind != was.kind || now.zone != was.zone || now.sky != was.sky {
            if let Some(step) = app.ui.map_edit.open.as_mut() {
                step.push(was);
            }
        }
    }

    /// A stroke on this page is not a step of the project's history: the map
    /// is not in the project. It is a step of its own, kept here.
    fn begin(&self, app: &mut App) {
        app.ui.map_edit.open = Some(Vec::new());
        app.ui.map_edit.redone.clear();
    }

    fn commit(&self, app: &mut App) {
        if let Some(step) = app.ui.map_edit.open.take() {
            if !step.is_empty() {
                app.ui.map_edit.steps.push(step);
                if app.ui.map_edit.steps.len() > STEPS_KEPT {
                    app.ui.map_edit.steps.remove(0);
                }
            }
        }
        settle(app);
    }

    /// Drawn through the camera like the sheet is, so the camera answers where
    /// a press landed.
    fn locate(
        &self,
        app: &App,
        _canvas: &HtmlCanvasElement,
        client_x: f64,
        client_y: f64,
    ) -> Option<(i32, i32)> {
        let (w, h) = self.dims(app)?;
        app.viewport.flat_cell_at(client_x, client_y, w, h)
    }
}

/// One cell as it stands. Read before a press and again after it, which is how
/// a stroke knows whether it changed anything worth putting back.
fn held(app: &App, at: usize) -> Was {
    let (kind, zone) = match app.settlement.as_ref() {
        Some(sim) => (
            sim.terrain.kind.get(at).copied().unwrap_or(0),
            sim.terrain.zone.get(at).copied().unwrap_or(0),
        ),
        None => (0, 0),
    };
    Was { at, kind, zone, sky: app.ui.map_edit.sky.get(at).copied().unwrap_or(0) }
}

/// What a stroke leaves for the rest of the program to do: one rebuild of the
/// plant index if the ground moved, the zones handed to the wilderness, and a
/// repaint.
fn settle(app: &mut App) {
    let ground = std::mem::take(&mut app.ui.map_edit.ground_dirty);
    if let Some(sim) = app.settlement.as_mut() {
        if ground {
            sim.rebuild_plant_index();
        }
        sim.sync_zones();
    }
    app.civ_stepped = true;
    app.civ_repaint();
    app.redraw_panel = true;
    crate::ui::sync_undo_buttons(app);
}

/// Whether there is a stroke on this page to put back, which is what the undo
/// button asks before it offers to.
pub fn can_undo(app: &App) -> bool {
    on_page(app) && !app.ui.map_edit.steps.is_empty()
}

pub fn can_redo(app: &App) -> bool {
    on_page(app) && !app.ui.map_edit.redone.is_empty()
}

fn on_page(app: &App) -> bool {
    app.mode == crate::app::Mode::Sprites && app.ui.tab == "map" && app.settlement.is_some()
}

/// Puts one stroke back, and hands the other way round to the redo list. True
/// if there was one, which is what tells the caller not to walk the project's
/// own history instead.
pub fn undo_stroke(app: &mut App) -> bool {
    take_step(app, false)
}

pub fn redo_stroke(app: &mut App) -> bool {
    take_step(app, true)
}

fn take_step(app: &mut App, forward: bool) -> bool {
    if !on_page(app) {
        return false;
    }
    let step = if forward {
        app.ui.map_edit.redone.pop()
    } else {
        app.ui.map_edit.steps.pop()
    };
    let step = match step {
        Some(step) => step,
        None => return false,
    };
    let (cols, rows) = match MapSurface::size(app) {
        Some(d) => d,
        None => return false,
    };
    let mut back: Step = Vec::with_capacity(step.len());
    // Backwards: a stroke that painted the same cell twice put the first of
    // them down first, so the last one to be undone is the first that happened.
    for was in step.into_iter().rev() {
        back.push(held(app, was.at));
        let (c, r) = ((was.at % cols as usize) as i32, (was.at / cols as usize) as i32);
        if r >= rows {
            continue;
        }
        if let Some(sim) = app.settlement.as_mut() {
            if sim.terrain.kind.get(was.at).copied().unwrap_or(0) != was.kind {
                sim.paint_cell(c, r, Cell::from_u8(was.kind));
                app.ui.map_edit.ground_dirty = true;
            }
        }
        if let Some(sim) = app.settlement.as_mut() {
            sim.terrain.set_zone(c, r, Zone::from_u8(was.zone));
        }
        if let Some(slot) = app.ui.map_edit.sky.get_mut(was.at) {
            *slot = was.sky;
        }
    }
    if forward {
        app.ui.map_edit.steps.push(back);
    } else {
        app.ui.map_edit.redone.push(back);
    }
    settle(app);
    true
}

/// The buffer the stage shows: the picture underneath, the map's own ground
/// over it, the zones over that, and the sky marks on top. One pixel per cell,
/// which the camera then scales to whatever zoom the page is at.
pub fn draw(app: &mut App) {
    let (w, h) = match MapSurface::size(app) {
        Some(d) => d,
        None => return draw_empty(app),
    };
    app.ui.map_edit.ensure(w, h);
    let tools = &app.ui.map_edit;
    let sim = match app.settlement.as_ref() {
        Some(sim) => sim,
        None => return draw_empty(app),
    };
    let over = if tools.image.is_some() { OVER_PICTURE } else { 1.0 };
    let mut buf = vec![0u32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            // The checker says "nothing under this" the same way it does under
            // a sprite, and the picture goes over it.
            let mut c = if (x + y) % 2 == 0 {
                crate::util::pack_rgba(26, 31, 38, 255)
            } else {
                crate::util::pack_rgba(20, 25, 32, 255)
            };
            if let Some((iw, ih, px)) = &tools.image {
                let sx = (((x as f64 + 0.5) / w as f64) * *iw as f64).floor() as i32;
                let sy = (((y as f64 + 0.5) / h as f64) * *ih as f64).floor() as i32;
                let sx = sx.clamp(0, iw - 1);
                let sy = sy.clamp(0, ih - 1);
                if let Some(&v) = px.get((sy * iw + sx) as usize) {
                    c = v;
                }
            }
            c = crate::util::mix_packed(c, Brush::of_ground(sim.terrain.type_at(x, y)).color(), over);
            let zone = sim.terrain.zone_at(x, y);
            if zone != Zone::Any {
                // Over the ground rather than instead of it: a zone is a
                // second thing said about a cell that already is something,
                // and hiding the ground would lose which.
                c = crate::util::mix_packed(c, Brush::of_zone(zone).color(), 0.55);
            }
            if tools.sky.get(i) == Some(&1) {
                c = crate::util::mix_packed(c, Brush::Sky.color(), 0.5);
            }
            buf[i] = c;
        }
    }
    app.viewport.present_flat(w, h, &buf);
    if app.viewport.show_grid {
        app.viewport.draw_pixel_grid(w, h);
    }
    app.viewport.finish();
}

/// No settlement, so no map: a checker the size of a small map, which is more
/// honest than an empty stage and gives the camera something to fit.
fn draw_empty(app: &mut App) {
    let (w, h) = (64, 32);
    let mut buf = vec![0u32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            buf[(y * w + x) as usize] = if (x + y) % 2 == 0 {
                crate::util::pack_rgba(26, 31, 38, 255)
            } else {
                crate::util::pack_rgba(20, 25, 32, 255)
            };
        }
    }
    app.viewport.present_flat(w, h, &buf);
    app.viewport.finish();
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    if let Some((cols, rows)) = MapSurface::size(app) {
        app.ui.map_edit.ensure(cols, rows);
    }

    append(
        root,
        section(
            "The map editor",
            vec![note(
                "The settlement's own map, drawn by hand. Paint what the land is with the \
                 tools above the stage - the same pencil, fill and eraser the sprite editor \
                 uses - and the map changes under the pointer: there is no draft and nothing \
                 to apply. Every stroke is one step back, kept for as long as this page is \
                 open. A picture can be laid under it to trace, or read straight in as the \
                 whole map.",
            )],
        ),
    );

    append(root, brushes_section(app, h));
    append(root, picture_section(app, h));

    let tally = el("div").class("stat-grid").get();
    append(
        root,
        section(
            "The map",
            vec![
                if app.settlement.is_some() {
                    note(
                        "What is standing on the map is left where it is: a cell somebody \
                         has built on keeps its ground whatever is painted over it. Zones are \
                         drawn nowhere but here - they say what may take root, which is a \
                         thing about the future of a cell rather than about how it looks.",
                    )
                } else {
                    // The page paints the settlement's map, and there is not
                    // one until somebody has been to the settlement. Rather
                    // than send them away, the button does what walking in
                    // there would have done.
                    btn_row(vec![{
                        // A plain button: founding the map is not a change to
                        // the project, so there is nothing for an undo step to
                        // put back.
                        let h2 = h.clone();
                        button("Make a map to paint on", Scope::Panel, move || {
                            let mut sh = h2.borrow_mut();
                            if sh.app.settlement.is_some() {
                                return;
                            }
                            sh.app.set_note("growing the wilderness...");
                            let civ = crate::civ::settlement::Settlement::new(&sh.app.state);
                            sh.app.settlement = Some(civ);
                            sh.app.pending_bootstrap = true;
                        })
                    }])
                },
                tally.clone(),
                danger_button("Take every zone off", Scope::Panel, {
                    let h2 = h.clone();
                    move || {
                        let mut sh = h2.borrow_mut();
                        if let Some(sim) = sh.app.settlement.as_mut() {
                            sim.terrain.zone.fill(0);
                            sim.sync_zones();
                        }
                        sh.app.ui.map_edit.steps.clear();
                        sh.app.ui.map_edit.redone.clear();
                        sh.app.civ_stepped = true;
                        sh.app.civ_repaint();
                        sh.app.rebuild_panel = true;
                    }
                }),
            ],
        ),
    );

    let mut panel = MapPanel { tally };
    panel.redraw(app);
    Box::new(panel)
}

/// The legend. Pressing one is choosing a color, which is all the pixel editor
/// underneath understands; what makes it a zone or a coastline is that the
/// color is one the legend can read back.
fn brushes_section(app: &App, h: &Handle) -> Element {
    let current = Brush::from_color(app.ui.brush_color);
    let mut rows = vec![note(
        "What the pointer paints. The first five are the ground itself, the next three are \
         zones drawn over it, and sky is a mark on this page alone. The eraser takes the zone \
         off a cell, or the sky mark off it, depending on which of them is chosen.",
    )];
    let chips = el("div").class("chips").get();
    for brush in BRUSHES {
        if brush == Brush::Clear {
            continue;
        }
        let swatch = el("span")
            .class("brush-dot")
            .style("background", &crate::util::packed_to_hex(brush.color()))
            .get();
        let class = if brush == current { "chip active" } else { "chip" };
        let h2 = h.clone();
        let chip = el("button")
            .class(class)
            .attr("type", "button")
            .attr("title", brush.hint())
            .attr("data-find", &crate::util::slug(brush.label()))
            .child(&swatch)
            .child(&el("span").text(brush.label()).get())
            .on("click", Scope::Panel, move |_| {
                let mut sh = h2.borrow_mut();
                crate::ui::color_wheel::set_brush(&mut sh.app, brush.color());
                sh.app.ui.tool = crate::app::Tool::Pencil;
                sh.app.rebuild_panel = true;
            })
            .get();
        let _ = chips.append_child(&chip);
    }
    rows.push(chips);
    rows.push(note(current.hint()));
    section("What to paint", rows)
}

/// The picture: under the map to trace, or read in as the map itself.
fn picture_section(app: &App, h: &Handle) -> Element {
    let mut rows = vec![note(
        "Drop a picture of a place and it is laid under the map, corner to corner, to trace \
         over. It is never part of the project and never part of a settlement: the picture \
         goes when the page does, and what is kept is the map painted with it there.",
    )];
    rows.push(drop_zone(h));
    let tools = &app.ui.map_edit;
    if let Some((iw, ih, _)) = &tools.image {
        rows.push(stat(
            if tools.name.is_empty() { "picture" } else { &tools.name },
            &format!("{iw} by {ih}"),
        ));
        let h2 = h.clone();
        rows.push(count_field(
            "Picture pixels to a cell",
            tools.scale() as f64,
            1.0,
            1.0,
            Some(
                "art drawn eight screen pixels to a pixel is eight here; it is guessed when \
                 the picture arrives, and it is what decides how large a map the picture makes",
            ),
            move |v| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.map_edit.px = (v as i32).max(1);
                sh.app.rebuild_panel = true;
            },
        ));
        if let Some((cols, rows_n)) = tools.picture_cells() {
            let cells = cols as f64 * rows_n as f64;
            let mut text = format!(
                "At that scale the picture is {cols} by {rows_n} cells, which is the map it \
                 would make."
            );
            if cells > 400_000.0 {
                text.push_str(
                    " That is a very large map: it costs memory for its pixel buffers and a \
                     long warmup for its wilderness, and nothing stops it.",
                );
            }
            rows.push(note(&text));
        }
        rows.push(btn_row(vec![
            app_button(h, "Use it as the map", use_as_map),
            app_button(h, "Take the sky colors", take_sky),
        ]));
        rows.push(note(
            "Using it as the map reads every cell straight out of the picture, nearest color \
             in the legend wins, and founds the settlement again on what comes out. Taking \
             the sky colors reads the top and the bottom of whatever is marked sky out of the \
             picture and sets the world's sky gradient to them.",
        ));
        let h2 = h.clone();
        rows.push(danger_button("Forget the picture", Scope::Panel, move || {
            let mut sh = h2.borrow_mut();
            sh.app.ui.map_edit.image = None;
            sh.app.ui.map_edit.name = String::new();
            sh.app.rebuild_panel = true;
        }));
    }
    section("A picture to trace", rows)
}

/// The picture as the whole map. The map takes the picture's own size at the
/// scale it was read at, with no ceiling on it: a drawing of a coastline is
/// worth however many cells it was drawn with.
fn use_as_map(app: &mut App) {
    let image = match app.ui.map_edit.image.clone() {
        Some(image) => image,
        None => {
            app.set_note("no picture to read a map out of");
            return;
        }
    };
    let (cols, rows) = match app.ui.map_edit.picture_cells() {
        Some(size) => size,
        None => return,
    };
    let cells = crate::civ::map_brush::read_picture(&image, cols, rows);
    app.record("map from a picture", false);
    app.state.civ.world.cols = cols;
    app.state.civ.world.rows = rows;
    app.ui.map_edit.sky = Vec::new();
    // The map is about to be exactly what these say, so nothing is left
    // waiting on Apply.
    app.civ_restart();
    app.pending_map = Some(cells);
    app.set_note(&format!("reading the picture as a {cols} by {rows} map..."));
    app.request_save();
}

/// The sky gradient, read out of the picture where the sky is marked. The top
/// of the band is the highest marked row and the horizon the lowest, which is
/// the way round a sky is.
fn take_sky(app: &mut App) {
    let image = match app.ui.map_edit.image.clone() {
        Some(image) => image,
        None => {
            app.set_note("no picture to read a sky out of");
            return;
        }
    };
    let (cols, rows) = match MapSurface::size(app) {
        Some(size) => size,
        None => {
            app.set_note("no map to mark a sky on: enter the settlement first");
            return;
        }
    };
    let (iw, ih, px) = image;
    let mut top: Option<(i32, u32)> = None;
    let mut bottom: Option<(i32, u32)> = None;
    for y in 0..rows {
        for x in 0..cols {
            if app.ui.map_edit.sky.get((y * cols + x) as usize) != Some(&1) {
                continue;
            }
            let sx = (((x as f64 + 0.5) / cols as f64) * iw as f64).floor() as i32;
            let sy = (((y as f64 + 0.5) / rows as f64) * ih as f64).floor() as i32;
            let v = match px.get((sy.clamp(0, ih - 1) * iw + sx.clamp(0, iw - 1)) as usize) {
                Some(&v) => v,
                None => continue,
            };
            if top.is_none_or(|(row, _)| y < row) {
                top = Some((y, v));
            }
            if bottom.is_none_or(|(row, _)| y > row) {
                bottom = Some((y, v));
            }
        }
    }
    match (top, bottom) {
        (Some((_, a)), Some((_, b))) => {
            app.record("sky-colors", false);
            app.state.civ.world.sky_top = crate::util::packed_to_hex(a);
            app.state.civ.world.sky_bottom = crate::util::packed_to_hex(b);
            app.request_save();
            app.civ_repaint();
            app.rebuild_panel();
            app.set_note("the sky is the picture's sky");
        }
        _ => app.set_note("nothing marked sky yet"),
    }
}

fn drop_zone(h: &Handle) -> Element {
    let picker = input_el("file").tap(|i| {
        i.set_accept("image/*");
        let _ = i.set_attribute("hidden", "hidden");
    });
    let zone = el("div").class("dropzone").get();
    let _ =
        zone.append_child(&el("span").class("dropzone-hint").text("Drop a picture to trace").get());
    let _ = zone.append_child(picker.unchecked_ref());

    for event in ["dragenter", "dragover"] {
        let zone2 = zone.clone();
        on(zone.unchecked_ref(), event, Scope::Panel, move |e: Event| {
            e.prevent_default();
            let _ = zone2.class_list().add_1("over");
        });
    }
    {
        let zone2 = zone.clone();
        on(zone.unchecked_ref(), "dragleave", Scope::Panel, move |_| {
            let _ = zone2.class_list().remove_1("over");
        });
    }
    {
        let zone2 = zone.clone();
        let h2 = h.clone();
        on(zone.unchecked_ref(), "drop", Scope::Panel, move |e: Event| {
            e.prevent_default();
            let _ = zone2.class_list().remove_1("over");
            let files = e
                .dyn_ref::<DragEvent>()
                .and_then(|d| d.data_transfer())
                .and_then(|t| t.files());
            if let Some(files) = files {
                take(&h2, files);
            }
        });
    }
    {
        let picker2 = picker.clone();
        on(zone.unchecked_ref(), "click", Scope::Panel, move |_| {
            picker2.click();
        });
    }
    {
        let h2 = h.clone();
        let picker2 = picker.clone();
        on(picker.unchecked_ref(), "change", Scope::Panel, move |_| {
            if let Some(files) = picker2.files() {
                take(&h2, files);
            }
            picker2.set_value("");
        });
    }
    let _ = zone.set_attribute("role", "button");
    let _ = zone.set_attribute("tabindex", "0");
    zone
}

fn take(h: &Handle, files: web_sys::FileList) {
    let name = files.get(0).map(|f| f.name()).unwrap_or_default();
    let h = h.clone();
    crate::ui::decode::read_files(files, move |frames, _, _| {
        let mut sh = h.borrow_mut();
        match frames.into_iter().next() {
            Some((w, height, px)) => {
                // Guessed rather than asked for. Art drawn at eight pixels to
                // a pixel is the common case and nobody wants to count them;
                // the number is on the panel to be changed when the guess is
                // wrong.
                let px_per = crate::civ::sprites::pixel_size(w, height, &px);
                sh.app.ui.map_edit.px = px_per;
                sh.app.ui.map_edit.image = Some((w, height, px));
                sh.app.ui.map_edit.name = name.clone();
                sh.app.set_note(&format!(
                    "{name} laid under the map at {px_per} px per cell"
                ));
            }
            None => sh.app.set_note("nothing readable in that drop"),
        }
        sh.app.rebuild_panel = true;
    });
}

pub struct MapPanel {
    tally: Element,
}

impl Panel for MapPanel {
    fn redraw(&mut self, app: &mut App) {
        crate::ui::clear(&self.tally);
        let sim = match app.settlement.as_ref() {
            Some(sim) => sim,
            None => {
                let _ = self
                    .tally
                    .append_child(&stat("no map yet", "enter the settlement to make one"));
                return;
            }
        };
        let (cols, rows) = (sim.world().cols, sim.world().rows);
        let total = (cols * rows).max(1) as f64;
        let _ = self.tally.append_child(&stat("cells", &format!("{cols} by {rows}")));
        // One pass for both grids rather than one per kind: this is redrawn at
        // the end of every stroke, and a map read out of a picture can be a
        // million cells.
        let mut ground = [0usize; 8];
        for &k in &sim.terrain.kind {
            if let Some(slot) = ground.get_mut(k as usize) {
                *slot += 1;
            }
        }
        let mut zones = [0usize; 8];
        for &z in &sim.terrain.zone {
            if let Some(slot) = zones.get_mut(z as usize) {
                *slot += 1;
            }
        }
        for kind in [Cell::Water, Cell::Grass, Cell::Sand, Cell::Rock, Cell::Cliff] {
            let n = ground[kind as usize];
            if n > 0 {
                let _ = self.tally.append_child(&stat(
                    Brush::of_ground(kind).label(),
                    &format!("{n} cells, {:.0}%", n as f64 / total * 100.0),
                ));
            }
        }
        for zone in [Zone::Wood, Zone::Low, Zone::Bare] {
            let n = zones[zone as usize];
            if n > 0 {
                let _ = self
                    .tally
                    .append_child(&stat(Brush::of_zone(zone).label(), &format!("{n} zoned")));
            }
        }
        let sky = app.ui.map_edit.marked_sky();
        if sky > 0 {
            let _ = self.tally.append_child(&stat("marked sky", &format!("{sky} cells")));
        }
    }
}
