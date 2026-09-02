//! The sprite editor: sheets, layers, frames, and the animation playing back
//! beside them.
//!
//! Everything here edits one sheet at a time, and which one, which layer of it
//! and which frame are all held in the shell's UI state rather than in the
//! panel, because the panel is thrown away and rebuilt whenever any of them
//! change. What the panel keeps is only the nodes it has to repaint: the
//! editor, the preview, and the thumbnails down the sides.

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Element, Event, HtmlCanvasElement};

use crate::app::{App, Handle, Panel, Tool};
use crate::art::{Sheet, MAX_LAYERS, MAX_SHEET_FRAMES, MAX_SHEET_PX};
use crate::civ::sprites::{FromSheet, MOTIONS};
use crate::ui::color_wheel::Brush;
use crate::ui::paint::Surface;
use crate::ui::{
    app_button, app_danger_button, app_num, app_text, append, btn_row, button, chip_head, clear,
    danger_button, el, note, on, section, window, NumOpts, Scope, Tap,
};
use crate::util::{packed_to_hex, EMPTY_COLOR};

const TOOLS: [(Tool, &str, &str); 5] = [
    (Tool::Pencil, "Pencil", "B"),
    (Tool::Eraser, "Eraser", "E"),
    (Tool::Fill, "Fill", "G"),
    (Tool::Pick, "Pick", "P"),
    (Tool::Select, "Marquee", "M"),
];

/// Most colors a sheet's palette row will show. Past this the row stops being
/// something to pick out of and starts being the sheet again.
const PALETTE_MAX: usize = 32;

// ---- what the editor draws on -------------------------------------------

/// The selected layer of the selected frame of the selected sheet. Drawing
/// lands on one layer; picking reads what is on show, which is the flattened
/// frame, because that is the color the pointer is over.
///
/// Public because the surface being drawn on is the stage, which the shell
/// owns: the sprite editor is a mode rather than a panel with a canvas in it.
pub struct SheetSurface;

fn selected(app: &App) -> Option<&Sheet> {
    app.state.art.find(&app.ui.selected_sheet)
}

impl Surface for SheetSurface {
    fn dims(&self, app: &App) -> Option<(i32, i32)> {
        selected(app).map(|s| (s.w, s.h))
    }

    fn get(&self, app: &App, x: i32, y: i32) -> u32 {
        match selected(app) {
            Some(s) => s.get(app.ui.sheet_layer, app.ui.sheet_frame, x, y),
            None => EMPTY_COLOR,
        }
    }

    fn set(&self, app: &mut App, x: i32, y: i32, v: u32) {
        let (id, layer, frame) = (
            app.ui.selected_sheet.clone(),
            app.ui.sheet_layer,
            app.ui.sheet_frame,
        );
        if let Some(s) = app.state.art.find_mut(&id) {
            s.set(layer, frame, x, y, v);
        }
    }

    fn pick(&self, app: &App, x: i32, y: i32) -> u32 {
        match selected(app) {
            Some(s) => {
                let flat = s.flatten(app.ui.sheet_frame);
                if x < 0 || y < 0 || x >= s.w || y >= s.h {
                    EMPTY_COLOR
                } else {
                    flat[(y * s.w + x) as usize]
                }
            }
            None => EMPTY_COLOR,
        }
    }

    fn commit(&self, app: &mut App) {
        app.art_changed();
    }

    /// The sheet is drawn through the camera rather than stretched to fill an
    /// element, so where a pointer landed is the camera's answer to give.
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

/// Runs an edit against the selected sheet and saves the result. The sheet is
/// taken by value so an edit can move frames and layers around without the
/// borrow checker having to be talked through it.
fn with_sheet(app: &mut App, f: impl FnOnce(&mut Sheet)) {
    let id = app.ui.selected_sheet.clone();
    let mut sheet = match app.state.art.find(&id) {
        Some(s) => s.clone(),
        None => return,
    };
    f(&mut sheet);
    sheet.fit();
    if let Some(slot) = app.state.art.find_mut(&id) {
        *slot = sheet;
    }
    app.clamp_selection();
    app.art_changed();
}

// ---- the panel -----------------------------------------------------------

pub struct ArtPanel {
    handle: Handle,
    brush: Option<Brush>,
    swatches: Option<Element>,
    frame_thumbs: Vec<(HtmlCanvasElement, i32)>,
    layer_thumbs: Vec<(HtmlCanvasElement, usize)>,
}

/// The drawing tab: what a stroke is made of, what is under it, and which frame
/// it lands in. The surface itself is the stage, so none of it is here.
pub fn build_draw(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    app.clamp_selection();
    if selected(app).is_none() {
        append(root, note("No sheets in this project. Add one on the Sheet tab."));
        return Box::new(crate::app::StaticPanel);
    }

    let mut brush = Brush::build(h, app);
    let swatches = el("div").class("swatches").get();
    let mut rows = vec![tool_row(app, h)];
    if let Some(row) = marquee_row(app, h) {
        rows.push(row);
    }
    rows.append(&mut brush.rows);
    rows.push(mirror_row(app, h));
    rows.push(swatches.clone());
    rows.push(image_drop(h));
    rows.push(crate::ui::decode::scale_field(
        app,
        h,
        "sheet",
        "how much of a dropped picture goes to one pixel of the sheet; 0 works it out from \
         the picture, and the default for every drop is on the Sheet tab",
    ));
    rows.push(nudge_row(h));
    rows.push(btn_row(vec![
        app_button(h, "Clear frame", |app| {
            let (layer, frame) = (app.ui.sheet_layer, app.ui.sheet_frame);
            with_sheet(app, |s| s.clear_cel(layer, frame));
        }),
        app_button(h, "Flip layer", |app| {
            let (layer, frame) = (app.ui.sheet_layer, app.ui.sheet_frame);
            with_sheet(app, |s| s.flip_cel(layer, frame));
        }),
        app_button(h, "Flip sheet", |app| with_sheet(app, |s| s.flip_all())),
    ]));
    append(root, section("Brush", rows));

    let mut panel = ArtPanel {
        handle: h.clone(),
        brush: Some(brush),
        swatches: Some(swatches),
        frame_thumbs: Vec::new(),
        layer_thumbs: Vec::new(),
    };
    // The thumbnails are made by the sections that own them, which run after
    // the panel exists so they can hand back the canvases.
    panel.layer_thumbs = layers_section(root, app, h);
    panel.frame_thumbs = frame_strip(root, app, h);
    if let Some(keys) = keys_section() {
        append(root, keys);
    }
    panel.redraw(app);
    Box::new(panel)
}

/// The keys the editor answers to, on anything with a keyboard to press them
/// with. Folded away, because it is a reference rather than a control.
fn keys_section() -> Option<Element> {
    if !crate::ui::has_keyboard() {
        return None;
    }
    let body = el("div").class("group-body").get();
    for (key, what) in crate::app::SPRITE_KEYS {
        let _ = body.append_child(
            &el("div")
                .class("stat")
                .child(&el("kbd").class("key").text(key).get())
                .child(&el("span").class("stat-val").text(what).get())
                .get(),
        );
    }
    Some(
        el("details")
            .class("group keys")
            .attr("data-group", "Keys")
            .child(
                &el("summary")
                    .class("group-head")
                    .child(&el("h3").text("Keys").get())
                    .get(),
            )
            .child(&body)
            .get(),
    )
}

/// The sheet tab: which sheets there are, how large and how fast this one is,
/// and where its art can be sent.
pub fn build_sheet(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    app.clamp_selection();
    append(root, sheet_section(app, h));
    if selected(app).is_some() {
        append(root, use_section(app, h));
    }
    append(root, imports_section(h));
    append(root, zip_section(app, h));
    append(root, store_section(app, h));
    Box::new(crate::app::StaticPanel)
}

/// How pictures are read on the way in, for every drop target in the tool.
/// Kept with the browser rather than with the project: it is about the art
/// somebody has on their disk, not about the thing being built.
fn imports_section(h: &Handle) -> Element {
    section(
        "Dropping pictures in",
        vec![
            note(
                "Art drawn large - eight screen pixels to a pixel, or sixteen - is read back \
                 down to the pixels it was drawn in, so a sheet holds what was drawn rather \
                 than a magnified copy of it. Every drop target starts at this and can be set \
                 to something else beside the target itself.",
            ),
            crate::ui::decode::default_scale_field(h),
        ],
    )
}

// ---- sections ------------------------------------------------------------

fn sheet_section(app: &App, h: &Handle) -> Element {
    let chips = el("div").class("chips").get();
    let _ = chips.append_child(&chip_head("Which sheet is on the easel"));
    for sheet in &app.state.art.sheets {
        let h2 = h.clone();
        let id = sheet.id.clone();
        let class = if sheet.id == app.ui.selected_sheet { "chip active" } else { "chip" };
        let chip = el("button")
            .class(class)
            .attr("type", "button")
            .text(&sheet.name)
            .on("click", Scope::Panel, move |_| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.selected_sheet = id.clone();
                sh.app.ui.sheet_layer = 0;
                sh.app.ui.sheet_frame = 0;
                sh.app.ui.playing = false;
                crate::app::fit_view(&mut sh.app);
                sh.app.rebuild_panel();
            })
            .get();
        let _ = chips.append_child(&chip);
    }

    let actions = btn_row(vec![
        app_button(h, "New sheet", |app| {
            let id = app.uid("art");
            let n = app.state.art.sheets.len() + 1;
            app.state.art.sheets.push(Sheet::new(&id, &format!("Sheet {n}"), 16, 16));
            app.ui.selected_sheet = id;
            app.ui.sheet_layer = 0;
            app.ui.sheet_frame = 0;
            app.art_changed();
            app.rebuild_panel();
        }),
        app_button(h, "Duplicate", |app| {
            let id = app.uid("art");
            let copy = match selected(app) {
                Some(s) => {
                    let mut copy = s.clone();
                    copy.id = id.clone();
                    copy.name = format!("{} copy", s.name);
                    copy
                }
                None => return,
            };
            app.state.art.sheets.push(copy);
            app.ui.selected_sheet = id;
            app.art_changed();
            app.rebuild_panel();
        }),
        app_danger_button(h, "Remove sheet", |app| {
            if app.state.art.sheets.len() <= 1 {
                return;
            }
            let id = app.ui.selected_sheet.clone();
            if let Some(i) = app.state.art.index_of(&id) {
                app.state.art.sheets.remove(i);
                let next = i.saturating_sub(1);
                app.ui.selected_sheet = app.state.art.sheets[next].id.clone();
                app.ui.sheet_layer = 0;
                app.ui.sheet_frame = 0;
                app.art_changed();
                app.rebuild_panel();
            }
        }),
    ]);

    let mut rows = vec![chips, actions];
    if let Some(sheet) = selected(app) {
        rows.push(app_text(h, "Name", &sheet.name, None, |app, v| {
            let id = app.ui.selected_sheet.clone();
            if let Some(s) = app.state.art.find_mut(&id) {
                s.name = v.to_string();
            }
            app.request_save();
        }));
        rows.push(app_num(h, "Frame width", sheet.w as f64,
            NumOpts { min: 1.0, max: MAX_SHEET_PX as f64, step: 1.0 },
            Some("art keeps its place; new room is empty"),
            |app, v| {
                let height = selected(app).map(|s| s.h).unwrap_or(1);
                with_sheet(app, |s| s.resize(v as i32, height));
                crate::app::fit_view(app);
                app.rebuild_panel();
            }));
        rows.push(app_num(h, "Frame height", sheet.h as f64,
            NumOpts { min: 1.0, max: MAX_SHEET_PX as f64, step: 1.0 }, None,
            |app, v| {
                let width = selected(app).map(|s| s.w).unwrap_or(1);
                with_sheet(app, |s| s.resize(width, v as i32));
                crate::app::fit_view(app);
                app.rebuild_panel();
            }));
        rows.push(app_num(h, "Rate", sheet.fps,
            NumOpts { min: 0.0, max: 24.0, step: 0.5 },
            Some("frames per second, for playing it back and for a clip made from this sheet"),
            |app, v| {
                let id = app.ui.selected_sheet.clone();
                if let Some(s) = app.state.art.find_mut(&id) {
                    s.fps = v;
                }
                app.art_changed();
            }));
        let per_cell = app.state.civ.art_px_per_cell.max(1.0);
        rows.push(note(&format!(
            "The settlement draws art at {per_cell:.0} pixels to a cell, so this frame stands \
             {:.1} by {:.1} cells there. A person is about a cell and a half tall; a house is \
             two or three cells across. Dropping a picture larger than the frame grows the \
             frame rather than shrinking the picture.",
            sheet.w as f64 / per_cell,
            sheet.h as f64 / per_cell
        )));
        rows.push(btn_row(vec![app_button(h, "Download PNG", download_sheet)]));
        rows.push(note(
            "One image, every frame side by side at one pixel each, which is the shape a \
             sheet is read back in.",
        ));
        let bytes = app.state.art.bytes();
        if bytes >= 1024 {
            rows.push(note(&format!(
                "Sheets in this project: {}.",
                if bytes >= 1 << 20 {
                    format!("{:.1} MB", bytes as f64 / (1 << 20) as f64)
                } else {
                    format!("{} kB", (bytes as f64 / 1024.0).round() as i64)
                }
            )));
        }
    }
    section("Sheets", rows)
}

/// Dropping images onto the sheet. One lands in the frame being drawn, several
/// fill successive frames from it, one each; either way they go on the layer
/// that is selected, so a reference can be dropped on a layer of its own and
/// drawn over on the one above.
fn image_drop(h: &Handle) -> Element {
    let picker = crate::ui::input_el("file").tap(|i| {
        i.set_accept("image/*");
        i.set_multiple(true);
        let _ = i.set_attribute("hidden", "hidden");
    });

    let zone = el("div").class("dropzone short").get();
    let _ = zone.append_child(
        &el("span")
            .class("dropzone-hint")
            .text("Drop images onto this layer, or click to choose")
            .get(),
    );
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
                .dyn_ref::<web_sys::DragEvent>()
                .and_then(|d| d.data_transfer())
                .and_then(|t| t.files());
            if let Some(files) = files {
                let h3 = h2.clone();
                crate::ui::decode::read_files(files, move |frames, _, _| {
                    place_images(&h3, frames);
                });
            }
        });
    }
    {
        let picker2 = picker.clone();
        on(zone.unchecked_ref(), "click", Scope::Panel, move |_| picker2.click());
    }
    {
        // The picker sits inside the zone, so the click that opens it would
        // bubble straight back to the handler that opened it.
        on(picker.unchecked_ref(), "click", Scope::Panel, |e: Event| e.stop_propagation());
    }
    {
        let picker2 = picker.clone();
        let h2 = h.clone();
        on(picker.unchecked_ref(), "change", Scope::Panel, move |_| {
            if let Some(files) = picker2.files() {
                let h3 = h2.clone();
                crate::ui::decode::read_files(files, move |frames, _, _| {
                    place_images(&h3, frames);
                });
            }
            picker2.set_value("");
        });
    }
    let _ = zone.set_attribute("role", "button");
    let _ = zone.set_attribute("tabindex", "0");
    zone
}

/// Lays what was dropped into the sheet, starting at the frame being drawn and
/// adding frames for anything that runs past the end of it.
fn place_images(h: &Handle, images: Vec<crate::civ::sprites::Frame>) {
    let mut sh = h.borrow_mut();
    if images.is_empty() {
        sh.app.set_note("nothing readable in that drop");
        return;
    }
    // Read down to the size it was drawn at before anything measures it: the
    // frame grows to hold what was dropped, and a picture handed over at eight
    // screen pixels to a pixel would grow it eight times too far.
    let want = crate::ui::decode::scale_of(&sh.app, "sheet");
    let (images, read_at) = crate::ui::decode::scaled(images, want);
    sh.app.record("drop", false);
    let start = sh.app.ui.sheet_frame;
    let layer = sh.app.ui.sheet_layer;
    let mut landed = start;
    let count = images.len();
    let want_w = images.iter().map(|f| f.0).max().unwrap_or(1).clamp(1, MAX_SHEET_PX);
    let want_h = images.iter().map(|f| f.1).max().unwrap_or(1).clamp(1, MAX_SHEET_PX);
    let mut grew = None;
    with_sheet(&mut sh.app, |sheet| {
        // The frame grows to hold what was dropped rather than the drop being
        // shrunk into it. Pixel art does not survive a resample, and the map
        // draws a frame at the size its own pixels say, so a picture that
        // arrives whole is a picture that comes out at the size it was drawn.
        if want_w > sheet.w || want_h > sheet.h {
            sheet.resize(sheet.w.max(want_w), sheet.h.max(want_h));
            grew = Some((sheet.w, sheet.h));
        }
        let mut at = start;
        for (w, h, px) in images {
            if at >= sheet.frame_count() {
                let next = sheet.add_frame(sheet.frame_count() - 1, false);
                if next < at {
                    // The sheet is at its cap and the rest have nowhere to go.
                    break;
                }
                at = next;
            }
            sheet.place(layer, at, w, h, &px);
            landed = at;
            at += 1;
        }
    });
    sh.app.ui.sheet_frame = landed;
    let mut placed = if count == 1 {
        "dropped onto this layer".to_string()
    } else {
        format!("{count} images across {} frames", landed - start + 1)
    };
    if read_at > 1 {
        placed.push_str(&format!(" at {read_at} px per pixel"));
    }
    sh.app.set_note(&match grew {
        Some((w, h)) => format!("{placed}; the frame grew to {w}x{h}"),
        None => placed,
    });
    if grew.is_some() {
        crate::app::fit_view(&mut sh.app);
    }
    sh.app.rebuild_panel = true;
}

/// Shifting the art in the frame being drawn by a pixel at a time, which is
/// what an animation needs far more often than it needs a selection: a pose
/// that is a pixel low, or a whole sheet that sits off center.
fn nudge_row(h: &Handle) -> Element {
    // A nudge does not rebuild the panel, so the switch beside the buttons is
    // read off the page rather than kept in the shell's state.
    let whole = crate::ui::check_button(false, "Every layer", Scope::Panel, |_| {});
    let mut buttons = Vec::new();
    for (label, dx, dy) in [
        ("Nudge left", -1, 0),
        ("Nudge right", 1, 0),
        ("Nudge up", 0, -1),
        ("Nudge down", 0, 1),
    ] {
        let h2 = h.clone();
        let whole = whole.clone();
        buttons.push(button(label, Scope::Panel, move || {
            let mut sh = h2.borrow_mut();
            sh.app.record(label, true);
            let (layer, frame) = (sh.app.ui.sheet_layer, sh.app.ui.sheet_frame);
            let every = crate::ui::pressed(&whole);
            // A selection wins over both: it is the smaller thing somebody
            // asked for, and dragging it out is a deliberate act.
            let picked = sh
                .app
                .sheet_dims()
                .and_then(|(w, h)| sh.app.ui.marquee_rect(w, h));
            with_sheet(&mut sh.app, |s| match (picked, every) {
                (Some(rect), _) => s.shift_region(layer, frame, rect, dx, dy),
                (None, true) => s.shift_all(dx, dy),
                (None, false) => s.shift_cel(layer, frame, dx, dy),
            });
        }));
    }
    let scope = el("label")
        .class("inline")
        .child(&whole)
        .child(&el("span").text("whole sheet").get())
        .get();
    el("div").class("btn-row").children(buttons).child(&scope).get()
}

fn tool_row(app: &App, h: &Handle) -> Element {
    let tools = el("div").class("btn-row").get();
    let keys = crate::ui::has_keyboard();
    for (tool, label, key) in TOOLS {
        let h2 = h.clone();
        let class = if app.ui.tool == tool { "btn active" } else { "btn" };
        // The key is on the button rather than only in a list, because the
        // button is where somebody is looking when they want the tool.
        let text = if keys { format!("{label} ({key})") } else { label.to_string() };
        let btn = el("button")
            .class(class)
            .attr("type", "button")
            .attr("data-find", &crate::ui::slug(label))
            .text(&text)
            .on("click", Scope::Panel, move |_| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.tool = tool;
                sh.app.rebuild_panel();
            })
            .get();
        let _ = tools.append_child(&btn);
    }
    tools
}

/// What the selection covers and the two things to do with it. Only shown when
/// there is one: with no selection the nudges act on the cel, which is what the
/// row below already says.
fn marquee_row(app: &App, h: &Handle) -> Option<Element> {
    let (w, h_px) = app.sheet_dims()?;
    let (x0, y0, x1, y1) = app.ui.marquee_rect(w, h_px)?;
    let said = format!(
        "{} by {} at {x0},{y0} - nudges and Clear act on this",
        x1 - x0 + 1,
        y1 - y0 + 1
    );
    let h2 = h.clone();
    let h3 = h.clone();
    Some(
        el("div")
            .class("marquee-row")
            .child(&el("span").class("field-hint").text(&said).get())
            .child(&btn_row(vec![
                button("Clear inside", Scope::Panel, move || {
                    let mut sh = h2.borrow_mut();
                    sh.app.record("clear selection", false);
                    let (layer, frame) = (sh.app.ui.sheet_layer, sh.app.ui.sheet_frame);
                    let rect = match sh.app.sheet_dims().and_then(|(w, h)| sh.app.ui.marquee_rect(w, h)) {
                        Some(r) => r,
                        None => return,
                    };
                    with_sheet(&mut sh.app, |s| s.clear_region(layer, frame, rect));
                }),
                button("Drop selection", Scope::Panel, move || {
                    let mut sh = h3.borrow_mut();
                    sh.app.ui.marquee = None;
                    sh.app.rebuild_panel();
                }),
            ]))
            .get(),
    )
}

fn mirror_row(app: &App, h: &Handle) -> Element {
    let h2 = h.clone();
    crate::ui::bool_field(
        "Mirror X",
        app.ui.mirror_x,
        Some("paints the same pixel on both sides"),
        move |on| h2.borrow_mut().app.ui.mirror_x = on,
    )
}

/// The layer stack, drawn top of the pile first the way it is looked at.
fn layers_section(root: &Element, app: &App, h: &Handle) -> Vec<(HtmlCanvasElement, usize)> {
    let sheet = match selected(app) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let list = el("div").class("layer-list").get();
    let mut thumbs = Vec::new();
    for (i, layer) in sheet.layers.iter().enumerate().rev() {
        let thumb = el("canvas")
            .class("thumb")
            .get()
            .dyn_into::<HtmlCanvasElement>()
            .unwrap();
        thumbs.push((thumb.clone(), i));

        let h2 = h.clone();
        let eye = crate::ui::check_button(
            layer.visible,
            "Layer shown",
            Scope::Panel,
            move |visible| {
                let mut sh = h2.borrow_mut();
                sh.app.record("layer visible", false);
                with_sheet(&mut sh.app, |s| {
                    if let Some(l) = s.layers.get_mut(i) {
                        l.visible = visible;
                    }
                });
            },
        );

        let name = crate::ui::input_el("text");
        name.set_value(&layer.name);
        {
            let h2 = h.clone();
            on(name.unchecked_ref(), "input", Scope::Panel, move |e| {
                let text = crate::ui::value_of(&e);
                let mut sh = h2.borrow_mut();
                sh.app.record("layer name", true);
                let id = sh.app.ui.selected_sheet.clone();
                if let Some(l) = sh.app.state.art.find_mut(&id).and_then(|s| s.layers.get_mut(i)) {
                    l.name = text;
                }
                sh.app.request_save();
            });
        }

        let h2 = h.clone();
        let class = if i == app.ui.sheet_layer { "layer-row active" } else { "layer-row" };
        let select = el("button")
            .class("layer-pick")
            .attr("type", "button")
            .attr("title", "Draw on this layer")
            .child(thumb.unchecked_ref())
            .on("click", Scope::Panel, move |_| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.sheet_layer = i;
                sh.app.rebuild_panel();
            })
            .get();

        let item = el("div")
            .class(class)
            .attr("draggable", "true")
            .attr("data-drag-at", &i.to_string())
            .attr("title", "Drag to move this layer up or down the stack")
            .child(&select)
            .child(name.unchecked_ref())
            .child(eye.unchecked_ref())
            .get();
        let _ = list.append_child(&item);
    }

    {
        let h2 = h.clone();
        crate::ui::reorder_by_drag(&list, Scope::Panel, move |from, to| {
            let mut sh = h2.borrow_mut();
            sh.app.record("reorder layers", false);
            let mut landed = to;
            with_sheet(&mut sh.app, |s| landed = s.drag_layer(from, to));
            sh.app.ui.sheet_layer = landed;
            sh.app.rebuild_panel();
        });
    }

    let actions = btn_row(vec![
        app_button(h, "Add layer", |app| {
            let at = app.ui.sheet_layer;
            let n = selected(app).map(|s| s.layers.len() + 1).unwrap_or(1);
            let name = format!("Layer {n}");
            let mut landed = at;
            with_sheet(app, |s| landed = s.add_layer(at, &name));
            app.ui.sheet_layer = landed;
            app.rebuild_panel();
        }),
        app_button(h, "Up", |app| {
            let at = app.ui.sheet_layer;
            let mut landed = at;
            with_sheet(app, |s| landed = s.move_layer(at, 1));
            app.ui.sheet_layer = landed;
            app.rebuild_panel();
        }),
        app_button(h, "Down", |app| {
            let at = app.ui.sheet_layer;
            let mut landed = at;
            with_sheet(app, |s| landed = s.move_layer(at, -1));
            app.ui.sheet_layer = landed;
            app.rebuild_panel();
        }),
        app_button(h, "Merge down", |app| {
            let at = app.ui.sheet_layer;
            let mut landed = at;
            with_sheet(app, |s| landed = s.merge_down(at));
            app.ui.sheet_layer = landed;
            app.rebuild_panel();
        }),
        app_danger_button(h, "Remove layer", |app| {
            let at = app.ui.sheet_layer;
            let mut landed = at;
            with_sheet(app, |s| landed = s.remove_layer(at));
            app.ui.sheet_layer = landed;
            app.rebuild_panel();
        }),
    ]);

    let full = sheet.layers.len() >= MAX_LAYERS;
    let mut rows = vec![list, actions];
    if full {
        rows.push(note(&format!("{MAX_LAYERS} layers is as deep as a sheet goes.")));
    }
    append(root, section("Layers", rows));
    thumbs
}

fn frame_strip(root: &Element, app: &App, h: &Handle) -> Vec<(HtmlCanvasElement, i32)> {
    let sheet = match selected(app) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let strip = el("div").class("frame-strip").get();
    let mut thumbs = Vec::new();
    for f in 0..sheet.frame_count() {
        let thumb = el("canvas")
            .class("frame-thumb")
            .get()
            .dyn_into::<HtmlCanvasElement>()
            .unwrap();
        thumbs.push((thumb.clone(), f));
        let h2 = h.clone();
        let class = if f == app.ui.sheet_frame { "frame-cell active" } else { "frame-cell" };
        let cell = el("button")
            .class(class)
            .attr("type", "button")
            .attr("title", &format!("Frame {}, or drag it somewhere else in the strip", f + 1))
            .attr("draggable", "true")
            .attr("data-drag-at", &f.to_string())
            .child(thumb.unchecked_ref())
            .child(&el("span").class("frame-index").text(&format!("{}", f + 1)).get())
            .on("click", Scope::Panel, move |_| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.sheet_frame = f;
                sh.app.ui.playing = false;
                sh.app.rebuild_panel();
            })
            .get();
        let _ = strip.append_child(&cell);
    }

    {
        let h2 = h.clone();
        crate::ui::reorder_by_drag(&strip, Scope::Panel, move |from, to| {
            let mut sh = h2.borrow_mut();
            sh.app.record("reorder frames", false);
            let mut landed = to as i32;
            with_sheet(&mut sh.app, |s| landed = s.drag_frame(from as i32, to as i32));
            sh.app.ui.sheet_frame = landed;
            sh.app.ui.playing = false;
            sh.app.rebuild_panel();
        });
    }

    let actions = btn_row(vec![
        app_button(h, "Add frame", |app| {
            let at = app.ui.sheet_frame;
            let mut landed = at;
            with_sheet(app, |s| landed = s.add_frame(at, false));
            app.ui.sheet_frame = landed;
            app.rebuild_panel();
        }),
        app_button(h, "Duplicate frame", |app| {
            let at = app.ui.sheet_frame;
            let mut landed = at;
            with_sheet(app, |s| landed = s.add_frame(at, true));
            app.ui.sheet_frame = landed;
            app.rebuild_panel();
        }),
        app_button(h, "Left", |app| {
            let at = app.ui.sheet_frame;
            let mut landed = at;
            with_sheet(app, |s| landed = s.move_frame(at, -1));
            app.ui.sheet_frame = landed;
            app.rebuild_panel();
        }),
        app_button(h, "Right", |app| {
            let at = app.ui.sheet_frame;
            let mut landed = at;
            with_sheet(app, |s| landed = s.move_frame(at, 1));
            app.ui.sheet_frame = landed;
            app.rebuild_panel();
        }),
        app_danger_button(h, "Remove frame", |app| {
            let at = app.ui.sheet_frame;
            let mut landed = at;
            with_sheet(app, |s| landed = s.remove_frame(at));
            app.ui.sheet_frame = landed;
            app.rebuild_panel();
        }),
    ]);

    let mut rows = vec![strip, actions];
    if sheet.frame_count() >= MAX_SHEET_FRAMES {
        rows.push(note(&format!("{MAX_SHEET_FRAMES} frames is as long as a sheet goes.")));
    }
    append(root, section("Frames", rows));
    thumbs
}

/// A block of packed pixels as PNG bytes, using the browser's own encoder: it
/// is right there, and a second one written here would be a deflate stream for
/// the sake of it.
fn png_bytes(w: i32, h: i32, px: &[u32]) -> Option<Vec<u8>> {
    if w <= 0 || h <= 0 || px.len() < (w * h) as usize {
        return None;
    }
    let canvas = crate::ui::document()
        .create_element("canvas")
        .ok()?
        .dyn_into::<HtmlCanvasElement>()
        .ok()?;
    canvas.set_width(w as u32);
    canvas.set_height(h as u32);
    let ctx = canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()?;
    // Straight into an ImageData: the buffer is already laid out as the RGBA
    // bytes a canvas wants, and the empty pixels have to stay transparent
    // rather than being painted over with a background.
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(px.as_ptr() as *const u8, px.len() * 4) };
    let image = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
        wasm_bindgen::Clamped(bytes),
        w as u32,
        h as u32,
    )
    .ok()?;
    let _ = ctx.put_image_data(&image, 0.0, 0.0);
    let url = canvas.to_data_url_with_type("image/png").ok()?;
    let base64 = url.split_once(",")?.1;
    crate::zip::from_base64(base64)
}

/// The selected sheet as a PNG: the strip, at one image pixel per art pixel,
/// which is both the honest size for an asset and the shape a drop zone reads
/// a sheet back in.
fn download_sheet(app: &mut App) {
    let sheet = match selected(app) {
        Some(s) => s,
        None => return,
    };
    let (w, h, px) = sheet.strip();
    let name = crate::ui::file_name(&sheet.name, "png");
    match png_bytes(w, h, &px) {
        Some(bytes) => {
            crate::ui::save_bytes(&bytes, "image/png", &name);
            app.set_note(&format!("saved {name}"));
        }
        None => app.set_note("the browser would not make a png of that"),
    }
}

/// One frame of a sheet as its own PNG, for art that is used a frame at a time
/// rather than as a strip.
fn download_frame(app: &mut App, frame: i32) {
    let (name, made) = match selected(app) {
        Some(sheet) => {
            let flat = sheet.flatten(frame);
            (
                crate::ui::file_name(&format!("{} {}", sheet.name, frame + 1), "png"),
                png_bytes(sheet.w, sheet.h, &flat),
            )
        }
        None => return,
    };
    match made {
        Some(bytes) => {
            crate::ui::save_bytes(&bytes, "image/png", &name);
            app.set_note(&format!("saved {name}"));
        }
        None => app.set_note("the browser would not make a png of that"),
    }
}

/// Everything ticked, in one archive: a strip per sheet, and a frame per file
/// beside it when that is asked for. Stored rather than compressed, since a
/// PNG is deflated already.
fn download_zip(app: &mut App) {
    /// One image on its way into the archive: what to call it, and the pixels.
    struct Image {
        name: String,
        w: i32,
        h: i32,
        px: Vec<u32>,
    }

    let want: Vec<(String, Vec<Image>)> = app
        .state
        .art
        .sheets
        .iter()
        .filter(|s| !app.ui.zip_skip.contains(&s.id))
        .map(|sheet| {
            let mut files = Vec::new();
            let (w, h, px) = sheet.strip();
            files.push(Image { name: crate::ui::file_name(&sheet.name, "png"), w, h, px });
            if app.ui.zip_frames {
                for f in 0..sheet.frame_count() {
                    files.push(Image {
                        name: crate::ui::file_name(&format!("{} {}", sheet.name, f + 1), "png"),
                        w: sheet.w,
                        h: sheet.h,
                        px: sheet.flatten(f),
                    });
                }
            }
            (crate::ui::file_name(&sheet.name, ""), files)
        })
        .collect();

    if want.is_empty() {
        app.set_note("nothing ticked to put in a zip");
        return;
    }

    let mut zip = crate::zip::Zip::new();
    let mut missed = 0;
    for (folder, files) in &want {
        for image in files {
            // A folder per sheet, so a zip of several does not land as a heap
            // of files whose names are all that tell them apart.
            match png_bytes(image.w, image.h, &image.px) {
                Some(bytes) => zip.add(&format!("{folder}/{}", image.name), &bytes),
                None => missed += 1,
            }
        }
    }
    if zip.is_empty() {
        app.set_note("nothing drawn on any of those sheets");
        return;
    }
    let n = zip.len();
    let name = crate::ui::file_name("grow sheets", "zip");
    crate::ui::save_bytes(&zip.finish(), "application/zip", &name);
    app.set_note(&if missed > 0 {
        format!("saved {name}: {n} images, {missed} the browser would not encode")
    } else {
        format!("saved {name}: {n} images")
    });
}

/// Which sheets go in the zip, and whether the frames go in one at a time.
/// None of this is the project, so none of it is recorded for undo.
fn zip_section(app: &App, h: &Handle) -> Element {
    let mut rows = vec![note(
        "One image per sheet, laid out as a strip. Tick the sheets to include; ask for frames \
         as well and each one lands beside the strip as its own file.",
    )];
    let list = el("div").class("chips").get();
    for sheet in &app.state.art.sheets {
        let id = sheet.id.clone();
        let taking = !app.ui.zip_skip.contains(&id);
        let h2 = h.clone();
        let chip = crate::ui::toggle_button(&sheet.name, taking, Scope::Panel, move |on| {
            let mut sh = h2.borrow_mut();
            sh.app.ui.zip_skip.retain(|s| *s != id);
            if !on {
                sh.app.ui.zip_skip.push(id.clone());
            }
        });
        let _ = list.append_child(&chip);
    }
    rows.push(list);

    let h2 = h.clone();
    rows.push(crate::ui::bool_field(
        "One file per frame too",
        app.ui.zip_frames,
        Some("as well as the strip, not instead of it"),
        move |on| h2.borrow_mut().app.ui.zip_frames = on,
    ));

    let h2 = h.clone();
    let h3 = h.clone();
    rows.push(btn_row(vec![
        button("Download this frame", Scope::Panel, move || {
            let mut sh = h2.borrow_mut();
            let frame = sh.app.ui.sheet_frame;
            download_frame(&mut sh.app, frame);
        }),
        button("Download zip", Scope::Panel, move || {
            let mut sh = h3.borrow_mut();
            download_zip(&mut sh.app);
        }),
    ]));
    section("Download", rows)
}

/// Sheets kept outside the project. The list is read from the store rather than
/// held, because it is changed by more than this panel: every save adds to it,
/// and a reset leaves it standing while everything else goes.
fn store_section(app: &App, h: &Handle) -> Element {
    let kept = crate::ui::sprite_store::load();
    let mut rows = vec![note(
        "Sheets are kept here as well as in the project, so they outlive the project they \
         were drawn in. Reset all leaves this alone: a kept sheet only goes when it is \
         deleted from here.",
    )];

    let prefs = crate::ui::prefs::Prefs::load();
    let h2 = h.clone();
    rows.push(crate::ui::bool_field(
        "Keep a copy",
        prefs.keep_sprites,
        Some("every save copies the project's sheets in here"),
        move |on| {
            let mut prefs = crate::ui::prefs::Prefs::load();
            prefs.keep_sprites = on;
            prefs.save();
            let mut sh = h2.borrow_mut();
            if prefs.keep_sprites {
                crate::ui::sprite_store::keep(&sh.app.state.art);
            }
            sh.app.rebuild_panel();
        },
    ));

    let list = el("div").class("kept-list").get();
    for sheet in &kept.sheets {
        let id = sheet.id.clone();
        let held = app.state.art.find(&id).is_some();
        let restore = {
            let h2 = h.clone();
            let id = id.clone();
            button(if held { "Replace" } else { "Restore" }, Scope::Panel, move || {
                let mut sh = h2.borrow_mut();
                let sheet = match crate::ui::sprite_store::find(&id) {
                    Some(s) => s,
                    None => return,
                };
                sh.app.record("restore sheet", false);
                match sh.app.state.art.index_of(&id) {
                    Some(at) => sh.app.state.art.sheets[at] = sheet,
                    None => sh.app.state.art.sheets.push(sheet),
                }
                sh.app.ui.selected_sheet = id.clone();
                sh.app.clamp_selection();
                sh.app.art_changed();
                sh.app.rebuild_panel();
            })
        };
        let drop = {
            let h2 = h.clone();
            let id = id.clone();
            let name = sheet.name.clone();
            danger_button("Delete", Scope::Panel, move || {
                // Deleting reaches outside the project, so undo cannot put it
                // back and the question has to be asked first.
                let asked = window()
                    .confirm_with_message(&format!(
                        "Delete the kept copy of {name}? Undo does not reach outside the \
                         project, so this cannot be taken back."
                    ))
                    .unwrap_or(false);
                if !asked {
                    return;
                }
                crate::ui::sprite_store::remove(&id);
                let mut sh = h2.borrow_mut();
                sh.app.set_note(&format!("deleted the kept copy of {name}"));
                sh.app.rebuild_panel();
            })
        };
        let frames = sheet.frame_count();
        let plural = if frames == 1 { "frame" } else { "frames" };
        let item = el("div")
            .class("kept-row")
            .child(
                &el("span")
                    .class("sampler-meta")
                    .child(&el("strong").text(&sheet.name).get())
                    .child(
                        &el("span")
                            .text(&format!("{}x{}, {frames} {plural}", sheet.w, sheet.h))
                            .get(),
                    )
                    .get(),
            )
            .child(&restore)
            .child(&drop)
            .get();
        let _ = list.append_child(&item);
    }
    if kept.sheets.is_empty() {
        rows.push(note("Nothing kept yet."));
    } else {
        rows.push(list);
        rows.push(note(&format!(
            "{} kept, {}.",
            kept.sheets.len(),
            size_text(crate::ui::sprite_store::bytes())
        )));
    }
    rows.push(btn_row(vec![app_button(h, "Keep these now", |app| {
        crate::ui::sprite_store::keep(&app.state.art);
        app.set_note("kept a copy of every sheet");
        app.rebuild_panel();
    })]));
    section("Kept sheets", rows)
}

fn size_text(bytes: usize) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1 << 20) as f64)
    } else if bytes >= 1024 {
        format!("{} kB", (bytes as f64 / 1024.0).round() as i64)
    } else {
        format!("{bytes} bytes")
    }
}

/// Pointing a person motion at this sheet, which is the whole reason the
/// editor is in a tool about a settlement.
fn use_section(app: &App, h: &Handle) -> Element {
    let sheet = selected(app);
    let ready = sheet.is_some_and(|s| s.any());
    let id = app.ui.selected_sheet.clone();
    let mut rows = vec![note(
        "Sends this sheet to a person motion as a clip. The clip keeps its own \
         copy, so the people on the map do not change again until it is sent \
         a second time.",
    )];
    // A motion this sheet is already behind says so on its own button: the
    // press is the same either way, and what it is worth is not.
    let mut behind = 0;
    let buttons: Vec<Element> = MOTIONS
        .iter()
        .map(|&motion| {
            let h2 = h.clone();
            let clip = app.state.civ.sprites.clip(motion).filter(|c| c.sheet == id);
            let state = clip.map(|c| c.against(sheet));
            let label = match state {
                Some(FromSheet::Current) => format!("{} - taken", motion.label()),
                Some(FromSheet::Behind) => {
                    behind += 1;
                    format!("{} - out of date", motion.label())
                }
                _ => motion.label().to_string(),
            };
            let node = button(&label, Scope::Panel, move || {
                let mut sh = h2.borrow_mut();
                let id = sh.app.ui.selected_sheet.clone();
                crate::ui::sprite_drop::build_from_sheet(&mut sh.app, motion, &id);
            });
            // The stamp names the motion, not the state: menu search points at
            // this button, and where it points must not depend on what has
            // been taken.
            let _ = node.set_attribute("data-find", &crate::ui::slug(motion.label()));
            if state == Some(FromSheet::Behind) {
                let _ = node.class_list().add_1("accent");
            }
            node
        })
        .collect();
    rows.push(btn_row(buttons));

    // The same sheet, sent to a thing people make rather than to a person.
    // There are thirty odd of those, so they are picked from a list.
    let slots = crate::civ::sprites::made_slots();
    if !slots.is_empty() {
        let options: Vec<(String, String)> = slots
            .iter()
            .map(|s| (s.id.clone(), format!("{} - {}", s.group, s.label)))
            .collect();
        let first = options[0].0.clone();
        let chosen = std::rc::Rc::new(std::cell::RefCell::new(first.clone()));
        let picker = {
            let chosen = chosen.clone();
            crate::ui::select_field("Or a made thing", &first, &options, None, move |v| {
                *chosen.borrow_mut() = v;
            })
        };
        let h2 = h.clone();
        let send = button("Use for that", Scope::Panel, move || {
            let target = chosen.borrow().clone();
            let mut sh = h2.borrow_mut();
            let id = sh.app.ui.selected_sheet.clone();
            match sh.app.state.art.find(&id).and_then(crate::civ::sprites::Clip::from_sheet) {
                Some(clip) => crate::ui::sprite_drop::apply_made(&mut sh.app, &target, clip),
                None => sh.app.set_note("nothing drawn on that sheet"),
            }
            sh.app.rebuild_panel();
        });
        rows.push(el("div").class("sprite-from").child(&picker).child(&send).get());
    }

    if behind > 0 {
        rows.push(note(&format!(
            "{behind} of these took this sheet before it was last drawn on. The people on the \
             map are still showing what it looked like then."
        )));
    }
    if !ready {
        rows.push(note("Nothing is drawn on this sheet yet."));
    }
    section("Use as person art", rows)
}

// ---- drawing -------------------------------------------------------------

/// Sets a canvas up for its own box at the display's pixel density and hands
/// back the context and the size in layout pixels.
fn fit_canvas(canvas: &HtmlCanvasElement) -> Option<(CanvasRenderingContext2d, f64, f64)> {
    let r = canvas.get_bounding_client_rect();
    let (rw, rh) = (r.width(), r.height());
    if rw <= 0.0 || rh <= 0.0 {
        return None;
    }
    let dpr = window().device_pixel_ratio();
    let w = ((rw * dpr).round() as u32).max(1);
    let h = ((rh * dpr).round() as u32).max(1);
    if canvas.width() != w || canvas.height() != h {
        canvas.set_width(w);
        canvas.set_height(h);
    }
    let ctx = canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()?;
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.clear_rect(0.0, 0.0, rw, rh);
    Some((ctx, rw, rh))
}

fn checker(ctx: &CanvasRenderingContext2d, x: f64, y: f64, cw: f64, ch: f64, dark: bool) {
    ctx.set_fill_style_str(if dark { "#141920" } else { "#1a1f26" });
    ctx.fill_rect(x, y, cw.ceil(), ch.ceil());
}

/// One frame at whatever size the box gives it, letterboxed so the art keeps
/// its shape. Used by the preview and by every thumbnail.
fn draw_frame(canvas: &HtmlCanvasElement, sheet: &Sheet, frame: i32, checkered: bool) {
    let (ctx, rw, rh) = match fit_canvas(canvas) {
        Some(v) => v,
        None => return,
    };
    let (gw, gh) = (sheet.w.max(1), sheet.h.max(1));
    let scale = (rw / gw as f64).min(rh / gh as f64);
    let (ox, oy) = ((rw - gw as f64 * scale) / 2.0, (rh - gh as f64 * scale) / 2.0);
    let flat = sheet.flatten(frame);
    for y in 0..gh {
        for x in 0..gw {
            let (px, py) = (ox + x as f64 * scale, oy + y as f64 * scale);
            if checkered {
                checker(&ctx, px, py, scale, scale, (x + y) % 2 == 0);
            }
            let v = flat[(y * gw + x) as usize];
            if v != EMPTY_COLOR {
                ctx.set_fill_style_str(&packed_to_hex(v));
                ctx.fill_rect(px, py, scale.ceil(), scale.ceil());
            }
        }
    }
}

/// One layer on its own, so the stack can be told apart at a glance.
fn draw_layer_thumb(canvas: &HtmlCanvasElement, sheet: &Sheet, layer: usize, frame: i32) {
    let (ctx, rw, rh) = match fit_canvas(canvas) {
        Some(v) => v,
        None => return,
    };
    let (gw, gh) = (sheet.w.max(1), sheet.h.max(1));
    let scale = (rw / gw as f64).min(rh / gh as f64);
    let (ox, oy) = ((rw - gw as f64 * scale) / 2.0, (rh - gh as f64 * scale) / 2.0);
    for y in 0..gh {
        for x in 0..gw {
            let v = sheet.get(layer, frame, x, y);
            if v == EMPTY_COLOR {
                continue;
            }
            ctx.set_fill_style_str(&packed_to_hex(v));
            ctx.fill_rect(ox + x as f64 * scale, oy + y as f64 * scale, scale.ceil(), scale.ceil());
        }
    }
}

/// Every color the sheet uses, the most used first, so the row reads as the
/// palette somebody has been working in rather than as a list of accidents.
fn palette(sheet: &Sheet) -> Vec<u32> {
    let mut tally: Vec<(u32, usize)> = Vec::new();
    for layer in &sheet.layers {
        for cel in &layer.cels {
            for v in &cel.px {
                if *v == EMPTY_COLOR {
                    continue;
                }
                match tally.iter_mut().find(|(c, _)| c == v) {
                    Some(entry) => entry.1 += 1,
                    None => tally.push((*v, 1)),
                }
            }
        }
    }
    tally.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    tally.into_iter().take(PALETTE_MAX).map(|(c, _)| c).collect()
}

impl Panel for ArtPanel {
    fn redraw(&mut self, app: &mut App) {
        let sheet = match selected(app) {
            Some(s) => s.clone(),
            None => return,
        };
        if let Some(brush) = &self.brush {
            brush.sync(app);
        }
        for (canvas, f) in &self.frame_thumbs {
            draw_frame(canvas, &sheet, *f, true);
        }
        for (canvas, layer) in &self.layer_thumbs {
            draw_layer_thumb(canvas, &sheet, *layer, app.ui.sheet_frame);
        }

        let swatches = match &self.swatches {
            Some(s) => s,
            None => return,
        };
        // The palette is rebuilt every redraw, so its listeners go in the scope
        // that is emptied first rather than piling up a closure a click.
        crate::ui::clear_scope(Scope::List);
        clear(swatches);
        for color in palette(&sheet) {
            let hex = packed_to_hex(color);
            let h2 = self.handle.clone();
            let sw = el("button")
                .class("swatch")
                .attr("type", "button")
                .attr("title", &hex)
                .style("background", &hex)
                .on("click", Scope::List, move |_| {
                    let mut sh = h2.borrow_mut();
                    crate::ui::color_wheel::set_brush(&mut sh.app, color);
                    sh.app.redraw_panel = true;
                })
                .get();
            let _ = swatches.append_child(&sw);
        }
    }
}
