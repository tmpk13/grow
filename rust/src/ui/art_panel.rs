//! The sprite editor: sheets, layers, frames, and the animation playing back
//! beside them.
//!
//! Everything here edits one sheet at a time, and which one, which layer of it
//! and which frame are all held in the shell's UI state rather than in the
//! panel, because the panel is thrown away and rebuilt whenever any of them
//! change. What the panel keeps is only the nodes it has to repaint: the
//! editor, the preview, and the thumbnails down the sides.

use std::rc::Rc;

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Element, HtmlCanvasElement};

use crate::app::{App, Handle, Panel, Tool};
use crate::art::{Sheet, MAX_LAYERS, MAX_SHEET_FRAMES, MAX_SHEET_PX};
use crate::civ::sprites::{Clip, Motion, MOTIONS};
use crate::ui::color_wheel::Brush;
use crate::ui::paint::{self, Surface};
use crate::ui::{
    app_button, app_danger_button, app_num, app_text, append, btn_row, button, clear, el, note,
    on, row, section, window, NumOpts, Scope,
};
use crate::util::{packed_to_hex, EMPTY_COLOR};

const TOOLS: [(Tool, &str); 4] = [
    (Tool::Pencil, "Pencil"),
    (Tool::Eraser, "Eraser"),
    (Tool::Fill, "Fill"),
    (Tool::Pick, "Pick"),
];

/// Most colors a sheet's palette row will show. Past this the row stops being
/// something to pick out of and starts being the sheet again.
const PALETTE_MAX: usize = 32;

// ---- what the editor draws on -------------------------------------------

/// The selected layer of the selected frame of the selected sheet. Drawing
/// lands on one layer; picking reads what is on show, which is the flattened
/// frame, because that is the color the pointer is over.
struct SheetSurface;

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
    clamp_selection(app);
    app.art_changed();
}

/// Keeps the layer and frame the panel is pointed at inside the sheet, which an
/// edit that removed either of them may have moved.
fn clamp_selection(app: &mut App) {
    let (layers, frames) = match selected(app) {
        Some(s) => (s.layers.len(), s.frame_count()),
        None => return,
    };
    app.ui.sheet_layer = app.ui.sheet_layer.min(layers.saturating_sub(1));
    app.ui.sheet_frame = app.ui.sheet_frame.clamp(0, frames - 1);
}

// ---- the panel -----------------------------------------------------------

pub struct ArtPanel {
    handle: Handle,
    editor: HtmlCanvasElement,
    preview: HtmlCanvasElement,
    brush: Brush,
    swatches: Element,
    frame_thumbs: Vec<(HtmlCanvasElement, i32)>,
    layer_thumbs: Vec<(HtmlCanvasElement, usize)>,
    play_label: Element,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    if selected(app).is_none() {
        app.ui.selected_sheet = app.state.art.sheets.first().map(|s| s.id.clone()).unwrap_or_default();
    }
    clamp_selection(app);

    append(root, sheet_section(app, h));

    let editor = el("canvas")
        .class("grid-canvas")
        .get()
        .dyn_into::<HtmlCanvasElement>()
        .unwrap();
    if let Some(s) = selected(app) {
        let wrap = el("div").class("editor-wrap").child(editor.unchecked_ref()).get();
        let _ = wrap
            .dyn_ref::<web_sys::HtmlElement>()
            .unwrap()
            .style()
            .set_property("aspect-ratio", &format!("{} / {}", s.w, s.h));
        paint::attach(
            &editor,
            h,
            Rc::new(SheetSurface),
            Rc::new(draw_editor),
        );

        let mut brush = Brush::build(h, app);
        let swatches = el("div").class("swatches").get();

        let mut rows = vec![tool_row(app, h)];
        rows.append(&mut brush.rows);
        rows.push(mirror_row(app, h));
        rows.push(swatches.clone());
        rows.push(wrap);
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
        append(root, section("Draw", rows));

        let (preview, play_label, animation) = animation_section(app, h);
        append(root, animation);

        let mut panel = ArtPanel {
            handle: h.clone(),
            editor,
            preview,
            brush,
            swatches,
            frame_thumbs: Vec::new(),
            layer_thumbs: Vec::new(),
            play_label,
        };
        // The thumbnails are made by the sections that own them, which run
        // after the panel exists so they can hand back the canvases.
        panel.layer_thumbs = layers_section(root, app, h);
        panel.frame_thumbs = frame_strip(root, app, h);
        append(root, use_section(app, h));
        panel.redraw(app);
        return Box::new(panel);
    }

    append(root, note("No sheets in this project. Add one to start drawing."));
    Box::new(crate::app::StaticPanel)
}

// ---- sections ------------------------------------------------------------

fn sheet_section(app: &App, h: &Handle) -> Element {
    let chips = el("div").class("chips").get();
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
                app.rebuild_panel();
            }));
        rows.push(app_num(h, "Frame height", sheet.h as f64,
            NumOpts { min: 1.0, max: MAX_SHEET_PX as f64, step: 1.0 }, None,
            |app, v| {
                let width = selected(app).map(|s| s.w).unwrap_or(1);
                with_sheet(app, |s| s.resize(width, v as i32));
                app.rebuild_panel();
            }));
        rows.push(app_num(h, "Rate", sheet.fps,
            NumOpts { min: 0.0, max: 24.0, step: 0.5 },
            Some("frames per second, for the preview and for a clip made from this sheet"),
            |app, v| {
                let id = app.ui.selected_sheet.clone();
                if let Some(s) = app.state.art.find_mut(&id) {
                    s.fps = v;
                }
                app.art_changed();
            }));
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

fn tool_row(app: &App, h: &Handle) -> Element {
    let tools = el("div").class("btn-row").get();
    for (tool, label) in TOOLS {
        let h2 = h.clone();
        let class = if app.ui.tool == tool { "btn active" } else { "btn" };
        let btn = el("button")
            .class(class)
            .attr("type", "button")
            .text(label)
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

fn mirror_row(app: &App, h: &Handle) -> Element {
    let mirror = crate::ui::input_el("checkbox");
    mirror.set_checked(app.ui.mirror_x);
    {
        let h2 = h.clone();
        on(mirror.unchecked_ref(), "change", Scope::Panel, move |e| {
            h2.borrow_mut().app.ui.mirror_x = crate::ui::checked_of(&e);
        });
    }
    row("Mirror X", mirror.unchecked_into(), Some("paints the same pixel on both sides"))
}

/// The preview and its transport. Hands back the preview canvas and the label
/// the frame counter is written into, so the panel can keep both up to date
/// without rebuilding itself.
fn animation_section(app: &App, h: &Handle) -> (HtmlCanvasElement, Element, Element) {
    let preview = el("canvas")
        .class("sheet-preview")
        .get()
        .dyn_into::<HtmlCanvasElement>()
        .unwrap();
    let wrap = el("div").class("preview-wrap tall").child(preview.unchecked_ref()).get();
    let play_label = el("span").class("readout").get();

    let play = {
        let h2 = h.clone();
        let text = if app.ui.playing { "Pause" } else { "Play" };
        el("button")
            .class("btn")
            .attr("type", "button")
            .text(text)
            .on("click", Scope::Panel, move |e| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.playing = !sh.app.ui.playing;
                sh.app.ui.play_time = 0.0;
                let text = if sh.app.ui.playing { "Pause" } else { "Play" };
                if let Some(node) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) {
                    node.set_text_content(Some(text));
                }
            })
            .get()
    };

    let onion = crate::ui::input_el("checkbox");
    onion.set_checked(app.ui.onion);
    {
        let h2 = h.clone();
        on(onion.unchecked_ref(), "change", Scope::Panel, move |e| {
            let mut sh = h2.borrow_mut();
            sh.app.ui.onion = crate::ui::checked_of(&e);
            sh.app.redraw_panel = true;
        });
    }

    let built = section(
        "Animation",
        vec![
            btn_row(vec![play, play_label.clone()]),
            row(
                "Onion skin",
                onion.unchecked_into(),
                Some("the frame before this one, faint, behind it"),
            ),
            wrap,
        ],
    );
    (preview, play_label, built)
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

        let eye = crate::ui::input_el("checkbox");
        eye.set_checked(layer.visible);
        {
            let h2 = h.clone();
            on(eye.unchecked_ref(), "change", Scope::Panel, move |e| {
                let visible = crate::ui::checked_of(&e);
                let mut sh = h2.borrow_mut();
                with_sheet(&mut sh.app, |s| {
                    if let Some(l) = s.layers.get_mut(i) {
                        l.visible = visible;
                    }
                });
            });
        }

        let name = crate::ui::input_el("text");
        name.set_value(&layer.name);
        {
            let h2 = h.clone();
            on(name.unchecked_ref(), "input", Scope::Panel, move |e| {
                let text = crate::ui::value_of(&e);
                let mut sh = h2.borrow_mut();
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
            .child(&select)
            .child(name.unchecked_ref())
            .child(eye.unchecked_ref())
            .get();
        let _ = list.append_child(&item);
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
            .attr("title", &format!("Frame {}", f + 1))
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

/// Pointing a settler motion at this sheet, which is the whole reason the
/// editor is in a tool about a settlement.
fn use_section(app: &App, h: &Handle) -> Element {
    let ready = selected(app).is_some_and(|s| s.any());
    let mut rows = vec![note(
        "Sends this sheet to a settler motion as a clip. The clip keeps its own \
         copy, so the settlers on the map do not change again until it is sent \
         a second time.",
    )];
    let buttons: Vec<Element> = MOTIONS
        .iter()
        .map(|&motion| {
            let h2 = h.clone();
            button(motion.label(), Scope::Panel, move || {
                let mut sh = h2.borrow_mut();
                send_to_motion(&mut sh.app, motion);
            })
        })
        .collect();
    rows.push(btn_row(buttons));
    if !ready {
        rows.push(note("Nothing is drawn on this sheet yet."));
    }
    section("Use as settler art", rows)
}

fn send_to_motion(app: &mut App, motion: Motion) {
    let clip = match selected(app).and_then(Clip::from_sheet) {
        Some(c) => c,
        None => {
            app.set_note("nothing drawn on that sheet");
            return;
        }
    };
    let mut clip = clip;
    // Playback that was tuned on this motion outlives the art it was tuned on.
    match app.state.civ.sprites.clip(motion) {
        Some(old) => {
            clip.stride = old.stride;
            clip.height = old.height;
            clip.lift = old.lift;
            clip.flip = old.flip;
            clip.mirror = old.mirror;
        }
        None => {
            let (fps, stride) = motion.playback();
            if clip.fps <= 0.0 {
                clip.fps = fps;
            }
            clip.stride = stride;
        }
    }
    let frames = clip.frame_count();
    app.state.civ.sprites.enabled = true;
    app.state.civ.sprites.set(motion, Some(clip));
    app.set_note(&format!("{}: {frames} frames from the editor", motion.label().to_lowercase()));
    app.sprites_changed();
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

/// The frame being drawn: the flattened stack, the frame before it behind that
/// when onion skin is on, and the grid over the top.
pub fn draw_editor(canvas: &HtmlCanvasElement, app: &App) {
    let sheet = match selected(app) {
        Some(s) => s,
        None => return,
    };
    let (ctx, rw, rh) = match fit_canvas(canvas) {
        Some(v) => v,
        None => return,
    };
    let (gw, gh) = (sheet.w, sheet.h);
    let cw = rw / gw as f64;
    let ch = rh / gh as f64;
    let frame = app.ui.sheet_frame;
    let flat = sheet.flatten(frame);
    let ghost = if app.ui.onion && sheet.frame_count() > 1 {
        Some(sheet.flatten((frame + sheet.frame_count() - 1) % sheet.frame_count()))
    } else {
        None
    };

    for y in 0..gh {
        for x in 0..gw {
            let i = (y * gw + x) as usize;
            let (px, py) = (x as f64 * cw, y as f64 * ch);
            checker(&ctx, px, py, cw, ch, (x + y) % 2 == 0);
            if let Some(ghost) = &ghost {
                let g = ghost[i];
                if g != EMPTY_COLOR && flat[i] == EMPTY_COLOR {
                    ctx.set_global_alpha(0.28);
                    ctx.set_fill_style_str(&packed_to_hex(g));
                    ctx.fill_rect(px, py, cw.ceil(), ch.ceil());
                    ctx.set_global_alpha(1.0);
                }
            }
            if flat[i] != EMPTY_COLOR {
                ctx.set_fill_style_str(&packed_to_hex(flat[i]));
                ctx.fill_rect(px, py, cw.ceil(), ch.ceil());
            }
        }
    }
    paint::cell_grid(&ctx, rw, rh, gw, gh, cw, ch);
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
        draw_editor(&self.editor, app);
        self.brush.sync(app);

        let frame = if app.ui.playing {
            play_frame(&sheet, app.ui.play_time)
        } else {
            app.ui.sheet_frame
        };
        draw_frame(&self.preview, &sheet, frame, false);
        self.play_label.set_text_content(Some(&format!(
            "frame {} of {}",
            frame + 1,
            sheet.frame_count()
        )));

        for (canvas, f) in &self.frame_thumbs {
            draw_frame(canvas, &sheet, *f, true);
        }
        for (canvas, layer) in &self.layer_thumbs {
            draw_layer_thumb(canvas, &sheet, *layer, app.ui.sheet_frame);
        }

        crate::ui::clear_scope(Scope::List);
        clear(&self.swatches);
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
            let _ = self.swatches.append_child(&sw);
        }
    }

    fn tick(&mut self, app: &mut App, dt: f64) {
        if !app.ui.playing {
            return;
        }
        let sheet = match selected(app) {
            Some(s) => s.clone(),
            None => return,
        };
        let before = play_frame(&sheet, app.ui.play_time);
        app.ui.play_time += dt;
        let after = play_frame(&sheet, app.ui.play_time);
        if before != after {
            draw_frame(&self.preview, &sheet, after, false);
            self.play_label.set_text_content(Some(&format!(
                "frame {} of {}",
                after + 1,
                sheet.frame_count()
            )));
        }
    }
}

fn play_frame(sheet: &Sheet, time: f64) -> i32 {
    let n = sheet.frame_count();
    if n <= 1 || sheet.fps <= 0.0 {
        return 0;
    }
    ((time * sheet.fps).floor() as i64).rem_euclid(n as i64) as i32
}
