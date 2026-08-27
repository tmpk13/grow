//! Drawable pixel grid used for every sampling box (and for the single shared
//! atlas). Works on whatever buffer the current materials mode exposes.

use std::rc::Rc;

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::app::{App, Handle};
use crate::sampler::MaterialMode;
use crate::ui::paint::{self, Surface};
use crate::ui::window;
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

/// The selected sampling box, or the shared atlas when that is the mode.
struct GridSurface;

impl Surface for GridSurface {
    fn dims(&self, app: &App) -> Option<(i32, i32)> {
        grid_dims(app)
    }

    fn get(&self, app: &App, x: i32, y: i32) -> u32 {
        grid_get(app, x, y)
    }

    fn set(&self, app: &mut App, x: i32, y: i32, v: u32) {
        grid_set(app, x, y, v)
    }

    fn commit(&self, app: &mut App) {
        app.materials_changed();
    }
}

pub struct GridEditor {
    pub canvas: HtmlCanvasElement,
}

impl GridEditor {
    pub fn attach(canvas: HtmlCanvasElement, h: &Handle) -> GridEditor {
        paint::attach(
            &canvas,
            h,
            Rc::new(GridSurface),
            Rc::new(draw),
        );
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

    paint::cell_grid(&ctx, rw, rh, gw, gh, cw, ch);

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
