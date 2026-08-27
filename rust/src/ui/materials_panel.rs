//! Materials panel: the sampling boxes, their layout mode and the pixel editor.

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Element, HtmlCanvasElement};

use crate::app::{App, Handle, Panel, Tool};
use crate::sampler::{role_def, role_label, Grid, MaterialMode, Region, Sampler, DEFAULT_TONES, ROLES};
use crate::ui::color_wheel::{set_brush, Brush};
use crate::ui::grid_editor::{self, GridEditor};
use crate::ui::{
    app_button, app_danger_button, app_num, app_select, app_text, append, btn_row, button, clear,
    clear_scope, el, input_el, on, section, window, NumOpts, Scope,
};
use crate::util::{hex_to_packed, mix_packed, packed_to_hex, EMPTY_COLOR};

const TOOLS: [(Tool, &str); 4] = [
    (Tool::Pencil, "Pencil"),
    (Tool::Eraser, "Eraser"),
    (Tool::Fill, "Fill"),
    (Tool::Pick, "Pick"),
];

pub struct MaterialsPanel {
    handle: Handle,
    editor: GridEditor,
    brush: Brush,
    swatches: Element,
    ramp_strip: Element,
    ramp_note: Element,
    thumbs: Vec<(HtmlCanvasElement, String)>,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    if app.state.materials.find(&app.ui.selected_sampler).is_none() {
        app.ui.selected_sampler = app
            .state
            .materials
            .samplers
            .first()
            .map(|s| s.id.clone())
            .unwrap_or_default();
    }
    let selected = app.ui.selected_sampler.clone();

    // ---- editor ----------------------------------------------------------
    let canvas = el("canvas")
        .class("grid-canvas")
        .get()
        .dyn_into::<HtmlCanvasElement>()
        .unwrap();
    let wrap = el("div").class("editor-wrap").child(canvas.unchecked_ref()).get();
    if let Some((gw, gh)) = grid_editor::grid_dims(app) {
        let _ = wrap
            .dyn_ref::<web_sys::HtmlElement>()
            .unwrap()
            .style()
            .set_property("aspect-ratio", &format!("{gw} / {gh}"));
    }
    let editor = GridEditor::attach(canvas, h);

    // ---- mode ------------------------------------------------------------
    let mode_options = vec![
        ("multi".to_string(), "Separate box per material".to_string()),
        ("single".to_string(), "One shared grid".to_string()),
    ];
    let mode_value = match app.state.materials.mode {
        MaterialMode::Multi => "multi",
        MaterialMode::Single => "single",
    };
    let mode_row = app_select(h, "Grid layout", mode_value, &mode_options,
        Some("switching to one grid copies the boxes into it"),
        |app, v| {
            let next = if v == "single" { MaterialMode::Single } else { MaterialMode::Multi };
            if next == app.state.materials.mode {
                return;
            }
            if next == MaterialMode::Single {
                app.state.materials.paint_atlas_from_samplers();
            }
            app.state.materials.mode = next;
            app.materials_changed();
            app.rebuild_panel();
        });

    let sync_buttons = btn_row(vec![
        app_button(h, "Boxes to shared grid", |app| {
            app.state.materials.paint_atlas_from_samplers();
            app.materials_changed();
            app.rebuild_panel();
        }),
        app_button(h, "Shared grid to boxes", |app| {
            app.state.materials.copy_atlas_to_samplers();
            app.materials_changed();
            app.rebuild_panel();
        }),
    ]);

    // ---- tools -----------------------------------------------------------
    let tool_buttons = el("div").class("btn-row").get();
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
        let _ = tool_buttons.append_child(&btn);
    }

    let mut brush = Brush::build(h, app);

    let mirror = input_el("checkbox");
    mirror.set_checked(app.ui.mirror_x);
    {
        let h2 = h.clone();
        on(mirror.unchecked_ref(), "change", Scope::Panel, move |e| {
            h2.borrow_mut().app.ui.mirror_x = crate::ui::checked_of(&e);
        });
    }

    let swatches = el("div").class("swatches").get();

    // ---- bulk ramp -------------------------------------------------------
    let ramp_a = input_el("color");
    ramp_a.set_value("#1d2b1a");
    let ramp_b = input_el("color");
    ramp_b.set_value("#9ed07a");
    let ramp_row = {
        let make = {
            let h2 = h.clone();
            let a = ramp_a.clone();
            let b = ramp_b.clone();
            button("Make ramp", Scope::Panel, move || {
                let dark = hex_to_packed(&a.value());
                let light = hex_to_packed(&b.value());
                let mut sh = h2.borrow_mut();
                with_region(&mut sh.app, |grid| fill_ramp(grid, dark, light));
                sh.app.materials_changed();
                sh.app.redraw_panel = true;
            })
        };
        let clear_btn = app_button(h, "Clear", |app| {
            with_region(app, |grid| grid.px.fill(EMPTY_COLOR));
            app.materials_changed();
            app.redraw_panel = true;
        });
        btn_row(vec![
            ramp_a.clone().unchecked_into(),
            ramp_b.clone().unchecked_into(),
            make,
            clear_btn,
        ])
    };

    let ramp_strip = el("div").class("ramp-strip").get();
    let ramp_note = el("p").class("note").get();

    let mut grid_rows = vec![mode_row, sync_buttons, atlas_settings(app, h), tool_buttons];
    grid_rows.append(&mut brush.rows);
    grid_rows.extend([
        crate::ui::row("Mirror X", mirror.unchecked_into(), None),
        swatches.clone(),
        wrap,
        ramp_row,
        ramp_strip.clone(),
        ramp_note.clone(),
    ]);
    append(root, section("Sampling grid", grid_rows));

    // ---- sampler list ----------------------------------------------------
    let list = el("div").class("sampler-list").get();
    let mut thumbs = Vec::new();
    for s in &app.state.materials.samplers {
        let thumb = el("canvas")
            .class("thumb")
            .get()
            .dyn_into::<HtmlCanvasElement>()
            .unwrap();
        thumbs.push((thumb.clone(), s.id.clone()));
        let h2 = h.clone();
        let id = s.id.clone();
        let class = if s.id == selected { "sampler-item active" } else { "sampler-item" };
        let item = el("button")
            .class(class)
            .attr("type", "button")
            .child(thumb.unchecked_ref())
            .child(
                &el("span")
                    .class("sampler-meta")
                    .child(&el("strong").text(&s.name).get())
                    .child(&el("span").text(role_label(&s.role)).get())
                    .get(),
            )
            .on("click", Scope::Panel, move |_| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.selected_sampler = id.clone();
                sh.app.rebuild_panel();
            })
            .get();
        let _ = list.append_child(&item);
    }

    let list_actions = btn_row(vec![
        app_button(h, "Add box", |app| {
            let index = app.state.materials.samplers.len();
            let band_y = (index as i32 * 2) % app.state.materials.atlas.h.max(1);
            let id = app.uid("mat");
            let atlas_w = app.state.materials.atlas.w;
            let mut s = Sampler::new(
                &id,
                &format!("Box {}", index + 1),
                "leaf",
                16,
                6,
                Region { x: 0, y: band_y, w: atlas_w, h: 2 },
            );
            if let Some(role) = role_def("leaf") {
                s.fill_default_art(role, index as i32 * 17, DEFAULT_TONES);
            }
            app.state.materials.samplers.push(s);
            app.ui.selected_sampler = id;
            app.materials_changed();
            app.rebuild_panel();
        }),
        app_danger_button(h, "Remove", |app| {
            if app.state.materials.samplers.len() <= 1 {
                return;
            }
            let selected = app.ui.selected_sampler.clone();
            if let Some(i) = app.state.materials.index_of(&selected) {
                app.state.materials.samplers.remove(i);
                let next = i.saturating_sub(1);
                app.ui.selected_sampler = app.state.materials.samplers[next].id.clone();
                app.materials_changed();
                app.rebuild_panel();
            }
        }),
    ]);

    append(root, section("Boxes", vec![list, list_actions, sampler_settings(app, h)]));

    let mut panel = MaterialsPanel {
        handle: h.clone(),
        editor,
        brush,
        swatches,
        ramp_strip,
        ramp_note,
        thumbs,
    };
    panel.redraw(app);
    Box::new(panel)
}

/// One row of the region form: label, current value, ceiling, and where it
/// writes back to.
type RegionField = (&'static str, f64, f64, fn(&mut Region, i32));

/// The atlas region of the selected box, or the whole grid in multi mode.
fn with_region(app: &mut App, f: impl FnOnce(&mut Grid)) {
    let id = app.ui.selected_sampler.clone();
    match app.state.materials.mode {
        MaterialMode::Multi => {
            if let Some(s) = app.state.materials.find_mut(&id) {
                let mut grid = Grid { w: s.w, h: s.h, px: std::mem::take(&mut s.px) };
                f(&mut grid);
                s.px = grid.px;
            }
        }
        MaterialMode::Single => {
            let region = match app.state.materials.find(&id) {
                Some(s) => s.region,
                None => return,
            };
            let mut grid = app.state.materials.patch(app.state.materials.find(&id).unwrap());
            f(&mut grid);
            for y in 0..grid.h {
                for x in 0..grid.w {
                    let v = grid.px[(y * grid.w + x) as usize];
                    app.state.materials.atlas.set(region.x + x, region.y + y, v);
                }
            }
        }
    }
}

fn fill_ramp(grid: &mut Grid, dark: u32, light: u32) {
    for y in 0..grid.h {
        for x in 0..grid.w {
            let t = if grid.w > 1 { x as f64 / (grid.w - 1) as f64 } else { 0.0 };
            let vy = if grid.h > 1 {
                (y as f64 / (grid.h - 1) as f64 - 0.5) * 0.12
            } else {
                0.0
            };
            grid.px[(y * grid.w + x) as usize] = mix_packed(dark, light, (t + vy).clamp(0.0, 1.0));
        }
    }
}

fn atlas_settings(app: &App, h: &Handle) -> Element {
    let holder = el("div").get();
    if app.state.materials.mode != MaterialMode::Single {
        return holder;
    }
    let atlas = &app.state.materials.atlas;
    let _ = holder.append_child(&app_num(h, "Shared grid width", atlas.w as f64,
        NumOpts { min: 2.0, max: 128.0, step: 1.0 }, None,
        |app, v| resize_atlas(app, (v as i32).max(2), app.state.materials.atlas.h)));
    let _ = holder.append_child(&app_num(h, "Shared grid height", atlas.h as f64,
        NumOpts { min: 2.0, max: 128.0, step: 1.0 }, None,
        |app, v| resize_atlas(app, app.state.materials.atlas.w, (v as i32).max(2))));
    holder
}

fn resize_atlas(app: &mut App, w: i32, h: i32) {
    let old = app.state.materials.atlas.clone();
    let mut next = Grid::new(w, h);
    for y in 0..h.min(old.h) {
        for x in 0..w.min(old.w) {
            next.set(x, y, old.get(x, y));
        }
    }
    app.state.materials.atlas = next;
    app.materials_changed();
    app.rebuild_panel();
}

fn sampler_settings(app: &App, h: &Handle) -> Element {
    let holder = el("div").get();
    let selected = match app.state.materials.find(&app.ui.selected_sampler) {
        Some(s) => s,
        None => return holder,
    };
    let id = selected.id.clone();

    let name_id = id.clone();
    let _ = holder.append_child(&app_text(h, "Name", &selected.name, None, move |app, v| {
        if let Some(s) = app.state.materials.find_mut(&name_id) {
            s.name = v.to_string();
        }
        app.rebuild_panel();
        app.request_save();
    }));

    let role_options: Vec<(String, String)> = ROLES
        .iter()
        .map(|r| (r.id.to_string(), r.label.to_string()))
        .collect();
    let role_id = id.clone();
    let _ = holder.append_child(&app_select(h, "Role", &selected.role, &role_options,
        Some("a label only; species pick boxes per material slot"),
        move |app, v| {
            if let Some(s) = app.state.materials.find_mut(&role_id) {
                s.role = v.to_string();
            }
            app.rebuild_panel();
            app.request_save();
        }));

    if app.state.materials.mode == MaterialMode::Multi {
        let width_id = id.clone();
        let _ = holder.append_child(&app_num(h, "Box width", selected.w as f64,
            NumOpts { min: 1.0, max: 64.0, step: 1.0 }, None,
            move |app, v| {
                if let Some(s) = app.state.materials.find_mut(&width_id) {
                    let hh = s.h;
                    s.resize((v as i32).max(1), hh);
                }
                app.materials_changed();
                app.rebuild_panel();
            }));
        let height_id = id.clone();
        let _ = holder.append_child(&app_num(h, "Box height", selected.h as f64,
            NumOpts { min: 1.0, max: 64.0, step: 1.0 }, None,
            move |app, v| {
                if let Some(s) = app.state.materials.find_mut(&height_id) {
                    let ww = s.w;
                    s.resize(ww, (v as i32).max(1));
                }
                app.materials_changed();
                app.rebuild_panel();
            }));
    } else {
        let atlas = &app.state.materials.atlas;
        let region = selected.region;
        let fields: [RegionField; 4] = [
            ("Region x", region.x as f64, (atlas.w - 1) as f64, |r, v| r.x = v.max(0)),
            ("Region y", region.y as f64, (atlas.h - 1) as f64, |r, v| r.y = v.max(0)),
            ("Region width", region.w as f64, atlas.w as f64, |r, v| r.w = v.max(1)),
            ("Region height", region.h as f64, atlas.h as f64, |r, v| r.h = v.max(1)),
        ];
        for (label, value, max, apply) in fields {
            let field_id = id.clone();
            let _ = holder.append_child(&app_num(h, label, value,
                NumOpts { min: 0.0, max, step: 1.0 }, None,
                move |app, v| {
                    if let Some(s) = app.state.materials.find_mut(&field_id) {
                        apply(&mut s.region, v as i32);
                    }
                    app.materials_changed();
                    app.rebuild_panel();
                }));
        }
    }
    holder
}

impl Panel for MaterialsPanel {
    fn redraw(&mut self, app: &mut App) {
        self.editor.draw(app);
        self.brush.sync(app);

        // The strip is the lookup shading actually reads, so a color covering
        // most of the box shows as most of the strip. The swatches below it are
        // the palette, one entry per color however little of the box it holds.
        let ramp = app.state.materials.ramp(&app.ui.selected_sampler);
        let lut = app.state.materials.tone_lut(&app.ui.selected_sampler);
        clear(&self.ramp_strip);
        for c in lut.iter() {
            let cell = el("span")
                .class("ramp-cell")
                .style("background", &packed_to_hex(*c))
                .get();
            let _ = self.ramp_strip.append_child(&cell);
        }
        self.ramp_note.set_text_content(Some(&format!(
            "{} tones, dark to light. Shading picks along this ramp, and each tone \
             holds as much of it as it covers of the box.",
            ramp.len()
        )));

        // The swatches are rebuilt every redraw, so their listeners go in the
        // scope that is emptied first rather than piling up a closure a click.
        clear_scope(Scope::List);
        clear(&self.swatches);
        for c in ramp.iter().rev() {
            let hex = packed_to_hex(*c);
            let value = *c;
            let h2 = self.handle.clone();
            let sw = el("button")
                .class("swatch")
                .attr("type", "button")
                .attr("title", &hex)
                .style("background", &hex)
                .on("click", Scope::List, move |_| {
                    let mut sh = h2.borrow_mut();
                    set_brush(&mut sh.app, value);
                    sh.app.redraw_panel = true;
                })
                .get();
            let _ = self.swatches.append_child(&sw);
        }

        for (canvas, id) in &self.thumbs {
            draw_thumb(canvas, app, id);
        }
    }

}

fn draw_thumb(canvas: &HtmlCanvasElement, app: &App, id: &str) {
    let sampler = match app.state.materials.find(id) {
        Some(s) => s,
        None => return,
    };
    let patch = app.state.materials.patch(sampler);
    let r = canvas.get_bounding_client_rect();
    let (rw, rh) = (r.width(), r.height());
    if rw == 0.0 {
        return;
    }
    let dpr = window().device_pixel_ratio();
    let w = ((rw * dpr).round() as u32).max(1);
    let h = ((rh * dpr).round() as u32).max(1);
    if canvas.width() != w || canvas.height() != h {
        canvas.set_width(w);
        canvas.set_height(h);
    }
    let ctx = match canvas.get_context("2d").ok().flatten() {
        Some(c) => c.dyn_into::<CanvasRenderingContext2d>().unwrap(),
        None => return,
    };
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.clear_rect(0.0, 0.0, rw, rh);
    let cw = rw / patch.w as f64;
    let ch = rh / patch.h as f64;
    for y in 0..patch.h {
        for x in 0..patch.w {
            let v = patch.px[(y * patch.w + x) as usize];
            ctx.set_fill_style_str(&if v == EMPTY_COLOR {
                "#12161c".to_string()
            } else {
                packed_to_hex(v)
            });
            ctx.fill_rect(x as f64 * cw, y as f64 * ch, cw.ceil(), ch.ceil());
        }
    }
}
