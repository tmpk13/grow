//! The map editor: a page of the sprite editor where a map is drawn by hand.
//!
//! It borrows the pixel editor whole. The stage is a grid one pixel per map
//! cell, the drawing tools are the ones already in the toolbar - pencil,
//! eraser, fill, pick, line, mirror - and a stroke is one step on the same
//! undo stack, because what is painted lives in the project. The only thing
//! this page adds is what the colors mean: the palette is a legend of zones
//! rather than a wheel, so picking a color is choosing what the land is.
//!
//! A picture can be laid under it to trace. The picture is not part of
//! anything: it is stretched over the draft corner to corner, it is kept for
//! as long as the page is open, and what survives is the drawing over it.
//!
//! Nothing here touches a running settlement until Apply, which walks the map
//! and asks the draft what each cell should be.

use wasm_bindgen::JsCast;
use web_sys::{DragEvent, Element, Event, HtmlCanvasElement};

use crate::app::{App, Handle, Panel};
use crate::civ::map_draft::{Brush, BRUSHES};
use crate::ui::paint::Surface;
use crate::ui::{
    app_button, append, btn_row, danger_button, el, input_el, note, number_field, on, section,
    stat, Scope, Tap, NumOpts,
};
use crate::util::EMPTY_COLOR;

/// The picture being traced. Not in the project and not in the settlement: a
/// tool held for as long as the page is open, the same contract the landscape
/// dropped on the Land panel has.
#[derive(Clone, Default)]
pub struct Tracing {
    pub image: Option<(i32, i32, Vec<u32>)>,
    pub name: String,
    /// How strongly it shows under the paint, nothing to fully.
    pub strength: f64,
}

/// The draft as a drawing surface. Colors in, brushes out: a color the legend
/// does not know is read as clear, which is what makes the eraser and a stray
/// color off the wheel both mean "say nothing about this cell".
pub struct MapSurface;

impl Surface for MapSurface {
    fn dims(&self, app: &App) -> Option<(i32, i32)> {
        let d = &app.state.civ.map_draft;
        if d.cols <= 0 || d.rows <= 0 {
            return None;
        }
        Some((d.cols, d.rows))
    }

    fn get(&self, app: &App, x: i32, y: i32) -> u32 {
        app.state.civ.map_draft.at(x, y).color()
    }

    fn set(&self, app: &mut App, x: i32, y: i32, v: u32) {
        let brush = if v == EMPTY_COLOR { Brush::Clear } else { Brush::from_color(v) };
        app.state.civ.map_draft.set(x, y, brush);
    }

    fn commit(&self, app: &mut App) {
        app.request_save();
        app.redraw_panel = true;
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

/// The buffer the stage shows: the picture underneath at whatever strength it
/// is set to, and the paint over it. One pixel per cell, which the camera then
/// scales to whatever zoom the page is at.
pub fn draw(app: &mut App) {
    let draft = &app.state.civ.map_draft;
    let (w, h) = (draft.cols.max(1), draft.rows.max(1));
    let trace = &app.ui.tracing;
    let strength = trace.strength.clamp(0.0, 1.0);
    let mut buf = vec![0u32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            // The checker says "nothing said here" the same way it does under
            // a sprite, and the picture goes over it.
            let mut c = if (x + y) % 2 == 0 {
                crate::util::pack_rgba(26, 31, 38, 255)
            } else {
                crate::util::pack_rgba(20, 25, 32, 255)
            };
            if let Some((iw, ih, px)) = &trace.image {
                let sx = (((x as f64 + 0.5) / w as f64) * *iw as f64).floor() as i32;
                let sy = (((y as f64 + 0.5) / h as f64) * *ih as f64).floor() as i32;
                let sx = sx.clamp(0, iw - 1);
                let sy = sy.clamp(0, ih - 1);
                if let Some(&v) = px.get((sy * iw + sx) as usize) {
                    c = crate::util::mix_packed(c, v, strength);
                }
            }
            let brush = draft.at(x, y);
            if brush != Brush::Clear {
                // Painted over the tracing rather than instead of it: a solid
                // color would hide the coastline being traced, and a wash
                // would not read as a decision. Most of the way is both.
                c = crate::util::mix_packed(c, brush.color(), 0.78);
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

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    app.state.civ.map_draft.ensure();

    append(
        root,
        section(
            "The map editor",
            vec![note(
                "A map drawn by hand instead of grown from noise. Paint what the land is with \
                 the tools above the stage - the same pencil, fill and eraser the sprite \
                 editor uses - and press Apply to lay it over the settlement's map. Every \
                 stroke is one step of undo, and the drawing is kept with the project.",
            )],
        ),
    );

    append(root, brushes_section(app, h));
    append(root, tracing_section(app, h));
    append(root, size_section(app, h));

    let tally = el("div").class("stat-grid").get();
    append(
        root,
        section(
            "Applying it",
            vec![
                note(
                    "The draft is stretched over the running map corner to corner, whatever \
                     size either of them is. Cells nothing was painted on are left exactly as \
                     they are, so a draft of one coastline can be laid over a generated map \
                     without flattening the rest of it. Anything already built on is left \
                     standing and counted as refused.",
                ),
                tally.clone(),
                btn_row(vec![
                    app_button(h, "Apply to the map", apply),
                    app_button(h, "Take the sky colors", take_sky),
                ]),
                note(
                    "Taking the sky colors reads the top and the bottom of whatever was \
                     painted sky out of the picture under it, and sets the world's sky \
                     gradient to them. It needs a picture; the paint alone says where the sky \
                     is, not what color it was.",
                ),
                danger_button("Wipe the drawing", Scope::Panel, {
                    let h2 = h.clone();
                    move || {
                        let mut sh = h2.borrow_mut();
                        sh.app.record("map-draft", false);
                        let n = sh.app.state.civ.map_draft.cells();
                        sh.app.state.civ.map_draft.paint = vec![0; n];
                        sh.app.request_save();
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
/// underneath understands; what makes it a zone is that the color is one the
/// draft can read back.
fn brushes_section(app: &App, h: &Handle) -> Element {
    let current = Brush::from_color(app.ui.brush_color);
    let mut rows = vec![note(
        "What the pointer paints. The eraser, and any color that is not one of these, says \
         nothing about a cell and leaves it to the map.",
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

/// The picture under the drawing.
fn tracing_section(app: &App, h: &Handle) -> Element {
    let mut rows = vec![note(
        "Drop a picture of a place and it is laid under the draft, corner to corner, to trace \
         over. It is never part of the project and never part of a settlement: the picture \
         goes when the page does, and what is kept is what was painted on top of it.",
    )];
    rows.push(drop_zone(h));
    if let Some((w, ih, _)) = &app.ui.tracing.image {
        rows.push(stat(
            if app.ui.tracing.name.is_empty() { "picture" } else { &app.ui.tracing.name },
            &format!("{w} by {ih}"),
        ));
        let h2 = h.clone();
        rows.push(number_field(
            "How strongly it shows",
            app.ui.tracing.strength,
            NumOpts { min: 0.0, max: 1.0, step: 0.05 },
            Some("turn it down to read the paint, up to read the picture"),
            move |v| {
                let mut sh = h2.borrow_mut();
                // The stage redraws every frame in this mode, so changing
                // what it shows needs nothing said to it.
                sh.app.ui.tracing.strength = v;
            },
        ));
        let h2 = h.clone();
        rows.push(danger_button("Forget the picture", Scope::Panel, move || {
            let mut sh = h2.borrow_mut();
            sh.app.ui.tracing.image = None;
            sh.app.ui.tracing.name = String::new();
            sh.app.rebuild_panel = true;
        }));
    }
    section("A picture to trace", rows)
}

/// How many cells the drawing is. Changing it throws the drawing away rather
/// than resampling it: a map redrawn at another size is a different map, and
/// silently stretching one would be a worse answer than asking again.
fn size_section(app: &App, h: &Handle) -> Element {
    let draft = &app.state.civ.map_draft;
    let (cols, rows_n) = (draft.cols, draft.rows);
    let painted = !draft.nothing_painted();
    let mut rows = vec![note(
        "The grid the map is drawn at. It does not have to match the settlement's own: the \
         draft is stretched over whatever map it is applied to.",
    )];
    let h2 = h.clone();
    rows.push(number_field(
        "Cells across",
        cols as f64,
        NumOpts { min: 16.0, max: 512.0, step: 1.0 },
        None,
        move |v| resize(&h2, v as i32, -1),
    ));
    let h2 = h.clone();
    rows.push(number_field(
        "Cells down",
        rows_n as f64,
        NumOpts { min: 8.0, max: 256.0, step: 1.0 },
        None,
        move |v| resize(&h2, -1, v as i32),
    ));
    if painted {
        rows.push(note("Changing either wipes the drawing."));
    }
    rows.push(btn_row(vec![app_button(h, "Match the settlement", |app| {
        let (cols, rows) = (app.state.civ.world.cols, app.state.civ.world.rows);
        if app.state.civ.map_draft.cols == cols && app.state.civ.map_draft.rows == rows {
            app.set_note("already the size of the map");
            return;
        }
        app.record("map-draft", false);
        app.state.civ.map_draft.cols = cols;
        app.state.civ.map_draft.rows = rows;
        app.state.civ.map_draft.paint = vec![0; (cols * rows) as usize];
        app.request_save();
        app.rebuild_panel();
    })]));
    section("Size", rows)
}

fn resize(h: &Handle, cols: i32, rows: i32) {
    let mut sh = h.borrow_mut();
    let sh = &mut *sh;
    let draft = &mut sh.app.state.civ.map_draft;
    let want = (if cols > 0 { cols } else { draft.cols }, if rows > 0 { rows } else { draft.rows });
    if (draft.cols, draft.rows) == want {
        return;
    }
    sh.app.record("map-draft", true);
    let draft = &mut sh.app.state.civ.map_draft;
    draft.cols = want.0;
    draft.rows = want.1;
    draft.paint = vec![0; (want.0 * want.1) as usize];
    sh.app.request_save();
    crate::app::fit_view(&mut sh.app);
}

/// Lays the draft over the running map. One pass over the map's own cells,
/// each asking the draft what it should be, so a small draft over a large map
/// is a blocky map rather than a partly painted one.
fn apply(app: &mut App) {
    let draft = app.state.civ.map_draft.clone();
    if draft.nothing_painted() {
        app.set_note("nothing painted yet");
        return;
    }
    let sim = match app.settlement.as_mut() {
        Some(sim) => sim,
        None => {
            app.set_note("no map to draw on: enter the settlement first");
            return;
        }
    };
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    // Gathered per answer and applied in batches, because both of the calls
    // below do one expensive pass at the end of a batch rather than one per
    // cell: painting a coastline a cell at a time rebuilds the plant index a
    // thousand times.
    let mut ground: Vec<(crate::civ::terrain::Cell, Vec<(i32, i32)>)> = Vec::new();
    let mut zones: Vec<(crate::world::Zone, Vec<(i32, i32)>)> = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let (dx, dy) = draft.cell_for(cols, rows, c, r);
            let brush = draft.at(dx, dy);
            if let Some(kind) = brush.ground() {
                match ground.iter_mut().find(|(k, _)| *k == kind) {
                    Some((_, cells)) => cells.push((c, r)),
                    None => ground.push((kind, vec![(c, r)])),
                }
            }
            if let Some(zone) = brush.zone() {
                match zones.iter_mut().find(|(z, _)| *z == zone) {
                    Some((_, cells)) => cells.push((c, r)),
                    None => zones.push((zone, vec![(c, r)])),
                }
            }
        }
    }
    let mut asked = 0;
    let mut done = 0;
    for (kind, cells) in &ground {
        asked += cells.len();
        done += sim.paint_cells(cells, *kind);
    }
    let mut zoned = 0;
    for (zone, cells) in &zones {
        zoned += sim.zone_cells(cells, *zone);
    }
    // A cell already the ground it was asked for is refused the same way a
    // built on one is, so only what was built on is worth naming.
    let refused = asked - done;
    app.set_note(&format!(
        "{done} cells drawn and {zoned} zoned{}",
        if refused > 0 {
            format!("; {refused} were already that or built on")
        } else {
            String::new()
        }
    ));
    app.civ_stepped = true;
    app.civ_repaint();
}

/// The sky gradient, read out of the picture where the sky was painted. The
/// top of the band is the highest sky row and the horizon the lowest, which is
/// the way round a sky is.
fn take_sky(app: &mut App) {
    let image = match app.ui.tracing.image.clone() {
        Some(image) => image,
        None => {
            app.set_note("no picture to read a sky out of");
            return;
        }
    };
    let draft = app.state.civ.map_draft.clone();
    let (iw, ih, px) = image;
    let mut top: Option<(i32, u32)> = None;
    let mut bottom: Option<(i32, u32)> = None;
    for y in 0..draft.rows {
        for x in 0..draft.cols {
            if draft.at(x, y) != Brush::Sky {
                continue;
            }
            let sx = (((x as f64 + 0.5) / draft.cols as f64) * iw as f64).floor() as i32;
            let sy = (((y as f64 + 0.5) / draft.rows as f64) * ih as f64).floor() as i32;
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
        _ => app.set_note("nothing painted sky yet"),
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
                sh.app.ui.tracing.image = Some((w, height, px));
                sh.app.ui.tracing.name = name.clone();
                if sh.app.ui.tracing.strength <= 0.0 {
                    sh.app.ui.tracing.strength = 0.7;
                }
                sh.app.set_note(&format!("{name} laid under the drawing"));
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
        let draft = &app.state.civ.map_draft;
        let total = draft.cells().max(1);
        let counts = draft.tally();
        if counts.is_empty() {
            let _ = self.tally.append_child(&stat("painted", "nothing yet"));
            return;
        }
        for (brush, n) in counts {
            let _ = self.tally.append_child(&stat(
                brush.label(),
                &format!("{n} cells, {:.0}%", n as f64 / total as f64 * 100.0),
            ));
        }
    }
}
