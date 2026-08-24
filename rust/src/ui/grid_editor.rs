//! Drawable pixel grid used for every sampling box (and for the single shared
//! atlas). Works on whatever buffer the current materials mode exposes.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Event, HtmlCanvasElement, PointerEvent};

use crate::app::{App, Handle, Tool};
use crate::sampler::MaterialMode;
use crate::ui::{on, window, Scope};
use crate::util::{packed_to_hex, EMPTY_COLOR};

/// The grid the editor is pointed at right now.
pub fn grid_dims(app: &App) -> Option<(i32, i32)> {
    match app.state.materials.mode {
        MaterialMode::Single => Some((app.state.materials.atlas.w, app.state.materials.atlas.h)),
        MaterialMode::Multi => app
            .state
            .materials
            .find(&app.ui.selected_sampler)
            .map(|s| (s.w, s.h)),
    }
}

pub fn grid_get(app: &App, x: i32, y: i32) -> u32 {
    match app.state.materials.mode {
        MaterialMode::Single => app.state.materials.atlas.get(x, y),
        MaterialMode::Multi => match app.state.materials.find(&app.ui.selected_sampler) {
            Some(s) => {
                if x < 0 || y < 0 || x >= s.w || y >= s.h {
                    EMPTY_COLOR
                } else {
                    s.px[(y * s.w + x) as usize]
                }
            }
            None => EMPTY_COLOR,
        },
    }
}

pub fn grid_set(app: &mut App, x: i32, y: i32, v: u32) {
    match app.state.materials.mode {
        MaterialMode::Single => app.state.materials.atlas.set(x, y, v),
        MaterialMode::Multi => {
            let id = app.ui.selected_sampler.clone();
            if let Some(s) = app.state.materials.find_mut(&id) {
                if x >= 0 && y >= 0 && x < s.w && y < s.h {
                    s.px[(y * s.w + x) as usize] = v;
                }
            }
        }
    }
}

fn flood_fill(app: &mut App, x: i32, y: i32, value: u32) {
    let (w, h) = match grid_dims(app) {
        Some(d) => d,
        None => return,
    };
    let target = grid_get(app, x, y);
    if target == value {
        return;
    }
    let mut stack = vec![(x, y)];
    while let Some((cx, cy)) = stack.pop() {
        if cx < 0 || cy < 0 || cx >= w || cy >= h {
            continue;
        }
        if grid_get(app, cx, cy) != target {
            continue;
        }
        grid_set(app, cx, cy, value);
        stack.push((cx - 1, cy));
        stack.push((cx + 1, cy));
        stack.push((cx, cy - 1));
        stack.push((cx, cy + 1));
    }
}

fn cell_at(canvas: &HtmlCanvasElement, app: &App, client_x: f64, client_y: f64) -> Option<(i32, i32)> {
    let (w, h) = grid_dims(app)?;
    let r = canvas.get_bounding_client_rect();
    if r.width() == 0.0 || r.height() == 0.0 {
        return None;
    }
    let x = ((client_x - r.left()) / r.width() * w as f64).floor() as i32;
    let y = ((client_y - r.top()) / r.height() * h as f64).floor() as i32;
    if x < 0 || y < 0 || x >= w || y >= h {
        return None;
    }
    Some((x, y))
}

fn apply(app: &mut App, cell: (i32, i32), erase: bool) {
    match app.ui.tool {
        Tool::Pick => {
            let v = grid_get(app, cell.0, cell.1);
            if v != EMPTY_COLOR {
                app.ui.brush_color = v;
                app.redraw_panel = true;
            }
        }
        Tool::Fill => {
            let value = if erase { EMPTY_COLOR } else { app.ui.brush_color };
            flood_fill(app, cell.0, cell.1, value);
        }
        _ => {
            let erase = erase || app.ui.tool == Tool::Eraser;
            let value = if erase { EMPTY_COLOR } else { app.ui.brush_color };
            grid_set(app, cell.0, cell.1, value);
            if app.ui.mirror_x {
                if let Some((w, _)) = grid_dims(app) {
                    grid_set(app, w - 1 - cell.0, cell.1, value);
                }
            }
        }
    }
}

fn stroke_line(app: &mut App, a: (i32, i32), b: (i32, i32), erase: bool) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs());
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let x = (a.0 as f64 + (b.0 - a.0) as f64 * t).round() as i32;
        let y = (a.1 as f64 + (b.1 - a.1) as f64 * t).round() as i32;
        apply(app, (x, y), erase);
    }
}

pub struct GridEditor {
    pub canvas: HtmlCanvasElement,
}

impl GridEditor {
    pub fn attach(canvas: HtmlCanvasElement, h: &Handle) -> GridEditor {
        let last: Rc<RefCell<Option<(i32, i32)>>> = Rc::new(RefCell::new(None));
        let drawing = Rc::new(RefCell::new(false));

        {
            let h2 = h.clone();
            let canvas2 = canvas.clone();
            let last = last.clone();
            let drawing = drawing.clone();
            on(canvas.unchecked_ref(), "pointerdown", Scope::Panel, move |e: Event| {
                let pe = e.dyn_ref::<PointerEvent>().unwrap();
                let _ = canvas2.set_pointer_capture(pe.pointer_id());
                let mut sh = h2.borrow_mut();
                let cell = cell_at(&canvas2, &sh.app, pe.client_x() as f64, pe.client_y() as f64);
                *drawing.borrow_mut() = true;
                *last.borrow_mut() = cell;
                if let Some(cell) = cell {
                    let erase = pe.buttons() & 2 == 2;
                    apply(&mut sh.app, cell, erase);
                    draw(&canvas2, &sh.app);
                }
            });
        }
        {
            let h2 = h.clone();
            let canvas2 = canvas.clone();
            let last = last.clone();
            let drawing = drawing.clone();
            on(canvas.unchecked_ref(), "pointermove", Scope::Panel, move |e: Event| {
                if !*drawing.borrow() {
                    return;
                }
                let pe = e.dyn_ref::<PointerEvent>().unwrap();
                let mut sh = h2.borrow_mut();
                let cell = match cell_at(&canvas2, &sh.app, pe.client_x() as f64, pe.client_y() as f64) {
                    Some(c) => c,
                    None => return,
                };
                let previous = *last.borrow();
                if previous == Some(cell) {
                    return;
                }
                let erase = pe.buttons() & 2 == 2;
                let freehand = matches!(sh.app.ui.tool, Tool::Pencil | Tool::Eraser);
                match previous {
                    Some(prev) if freehand => stroke_line(&mut sh.app, prev, cell, erase),
                    _ => apply(&mut sh.app, cell, erase),
                }
                *last.borrow_mut() = Some(cell);
                draw(&canvas2, &sh.app);
            });
        }
        for event in ["pointerup", "pointercancel", "pointerleave"] {
            let h2 = h.clone();
            let last = last.clone();
            let drawing = drawing.clone();
            on(canvas.unchecked_ref(), event, Scope::Panel, move |_| {
                if !*drawing.borrow() {
                    return;
                }
                *drawing.borrow_mut() = false;
                *last.borrow_mut() = None;
                let mut sh = h2.borrow_mut();
                sh.app.materials_changed();
                sh.app.redraw_panel = true;
            });
        }
        {
            let canvas2 = canvas.clone();
            on(canvas2.unchecked_ref(), "contextmenu", Scope::Panel, move |e: Event| {
                e.prevent_default();
            });
        }

        GridEditor { canvas }
    }

    pub fn draw(&self, app: &App) {
        draw(&self.canvas, app);
    }
}

/// One cell per pixel, plus the region outlines when the shared grid is on.
pub fn draw(canvas: &HtmlCanvasElement, app: &App) {
    let (gw, gh) = match grid_dims(app) {
        Some(d) => d,
        None => return,
    };
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
    let cw = rw / gw as f64;
    let ch = rh / gh as f64;

    for y in 0..gh {
        for x in 0..gw {
            let v = grid_get(app, x, y);
            if v == EMPTY_COLOR {
                ctx.set_fill_style_str(if (x + y) % 2 == 0 { "#1a1f26" } else { "#141920" });
            } else {
                ctx.set_fill_style_str(&packed_to_hex(v));
            }
            ctx.fill_rect(x as f64 * cw, y as f64 * ch, cw.ceil(), ch.ceil());
        }
    }

    if cw.min(ch) >= 7.0 {
        ctx.set_stroke_style_str("rgba(255,255,255,0.07)");
        ctx.set_line_width(1.0);
        ctx.begin_path();
        for x in 1..gw {
            let px = (x as f64 * cw).round() + 0.5;
            ctx.move_to(px, 0.0);
            ctx.line_to(px, rh);
        }
        for y in 1..gh {
            let py = (y as f64 * ch).round() + 0.5;
            ctx.move_to(0.0, py);
            ctx.line_to(rw, py);
        }
        ctx.stroke();
    }

    if app.state.materials.mode != MaterialMode::Single {
        return;
    }
    for s in &app.state.materials.samplers {
        let active = s.id == app.ui.selected_sampler;
        let color = if active {
            "rgba(255,210,120,0.95)"
        } else {
            "rgba(255,255,255,0.35)"
        };
        ctx.set_stroke_style_str(color);
        ctx.set_line_width(if active { 2.0 } else { 1.0 });
        ctx.stroke_rect(
            s.region.x as f64 * cw + 1.0,
            s.region.y as f64 * ch + 1.0,
            s.region.w as f64 * cw - 2.0,
            s.region.h as f64 * ch - 2.0,
        );
        ctx.set_fill_style_str(color);
        ctx.set_font("0.7rem system-ui, sans-serif");
        let _ = ctx.fill_text(&s.name, s.region.x as f64 * cw + 4.0, s.region.y as f64 * ch + 12.0);
    }
}
