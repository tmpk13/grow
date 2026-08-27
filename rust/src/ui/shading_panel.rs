//! Shading panel: the shared tone curve, plotted and previewed on test shapes.

use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, Element, HtmlCanvasElement, ImageData};

use crate::app::{App, Handle, Panel};
use crate::shading::{curve_value, quantize, shade_value, Shading};
use crate::ui::{
    app_button, app_num, append, btn_row, el, note, sampler_options, section, window,
    NumOpts,
};
use crate::util::{clamp01, distance_transform, label_components, unpack_rgba};

pub struct ShadingPanel {
    plot: HtmlCanvasElement,
    preview: HtmlCanvasElement,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    let plot = canvas("plot-canvas");
    let preview = canvas("shade-preview");

    let s = app.state.shading;
    let fields = vec![
        shade_num(h, "Mid tone", s.mid, 0.0, 1.0, 0.01, Some("tone before any shading is applied"), |s, v| s.mid = v),
        shade_num(h, "Center darker", s.center_dark, 0.0, 1.0, 0.01, Some("pixels deep inside a shape"), |s, v| s.center_dark = v),
        shade_num(h, "Top edge lighter", s.top_light, 0.0, 1.0, 0.01, None, |s, v| s.top_light = v),
        shade_num(h, "Bottom edge darker", s.bottom_dark, 0.0, 1.0, 0.01, None, |s, v| s.bottom_dark = v),
        shade_num(h, "Curve start", s.edge0, 0.0, 1.0, 0.01, Some("below this the response is flat at 0"), |s, v| s.edge0 = v),
        shade_num(h, "Curve end", s.edge1, 0.0, 1.0, 0.01, Some("above this the response is flat at 1"), |s, v| s.edge1 = v),
        shade_num(h, "Curve gamma", s.gamma, 0.2, 4.0, 0.05, Some("below 1 reaches the plateau sooner"), |s, v| s.gamma = v),
    ];

    let presets = btn_row(vec![
        app_button(h, "Flat body", |app| preset(app, 0.05, 0.3, 1.0)),
        app_button(h, "Soft", |app| preset(app, 0.0, 1.0, 1.0)),
        app_button(h, "Rim only", |app| preset(app, 0.55, 0.95, 1.4)),
        app_button(h, "Reset", |app| {
            app.state.shading = Shading::default();
            app.shading_changed();
            app.rebuild_panel();
        }),
    ]);

    let mut curve = vec![el("div").class("plot-wrap").child(plot.unchecked_ref()).get()];
    curve.push(note(
        "x is the input (depth inside the shape, or distance from an edge), y is the curve \
         response. A narrow start-to-end span leaves most of a body on one flat tone.",
    ));
    curve.extend(fields);
    curve.push(presets);
    append(root, section("Tone curve", curve));

    // These three describe the preview rather than the project, so they go
    // through the plain fields: recording them would put steps on the undo
    // stack that change nothing when they are taken back.
    let preview_fields = vec![
        el("div").class("preview-wrap").child(preview.unchecked_ref()).get(),
        {
            let h2 = h.clone();
            crate::ui::select_field("Preview material", &app.ui.shade_preview_sampler.clone(),
                &sampler_options(app), None, move |v| {
                    let mut sh = h2.borrow_mut();
                    sh.app.ui.shade_preview_sampler = v;
                    sh.app.redraw_panel = true;
                })
        },
        {
            let h2 = h.clone();
            crate::ui::number_field("Preview tone steps", app.ui.shade_preview_tones as f64,
                NumOpts { min: 2.0, max: 16.0, step: 1.0 }, None, move |v| {
                    let mut sh = h2.borrow_mut();
                    sh.app.ui.shade_preview_tones = v as i32;
                    sh.app.redraw_panel = true;
                })
        },
        {
            let h2 = h.clone();
            crate::ui::number_field("Preview core depth (px)", app.ui.shade_preview_core,
                NumOpts { min: 0.5, max: 16.0, step: 0.5 },
                Some("depth at which a shape reads as fully core"), move |v| {
                    let mut sh = h2.borrow_mut();
                    sh.app.ui.shade_preview_core = v;
                    sh.app.redraw_panel = true;
                })
        },
    ];
    append(root, section("Preview", preview_fields));

    let mut panel = ShadingPanel { plot, preview };
    panel.redraw(app);
    Box::new(panel)
}

fn preset(app: &mut App, edge0: f64, edge1: f64, gamma: f64) {
    app.state.shading.edge0 = edge0;
    app.state.shading.edge1 = edge1;
    app.state.shading.gamma = gamma;
    app.shading_changed();
    app.rebuild_panel();
}

fn canvas(class: &str) -> HtmlCanvasElement {
    el("canvas").class(class).get().dyn_into::<HtmlCanvasElement>().unwrap()
}

#[allow(clippy::too_many_arguments)]
fn shade_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut Shading, f64),
) -> Element {
    app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.shading, v);
        app.redraw_panel = true;
        app.shading_changed();
    })
}

impl Panel for ShadingPanel {
    fn redraw(&mut self, app: &mut App) {
        draw_curve(&self.plot, &app.state.shading);
        draw_shapes(&self.preview, app);
    }
}

fn context(canvas: &HtmlCanvasElement) -> Option<(CanvasRenderingContext2d, f64, f64)> {
    let ctx = canvas
        .get_context("2d")
        .ok()
        .flatten()?
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()?;
    let r = canvas.get_bounding_client_rect();
    if r.width() == 0.0 {
        return None;
    }
    let dpr = window().device_pixel_ratio();
    canvas.set_width((r.width() * dpr).round() as u32);
    canvas.set_height((r.height() * dpr).round() as u32);
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.set_fill_style_str("#0b0f14");
    ctx.fill_rect(0.0, 0.0, r.width(), r.height());
    Some((ctx, r.width(), r.height()))
}

fn draw_curve(canvas: &HtmlCanvasElement, shading: &Shading) {
    let (ctx, w, h) = match context(canvas) {
        Some(c) => c,
        None => return,
    };

    ctx.set_stroke_style_str("rgba(255,255,255,0.08)");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    for i in 1..4 {
        let x = (w * i as f64) / 4.0;
        let y = (h * i as f64) / 4.0;
        ctx.move_to(x, 0.0);
        ctx.line_to(x, h);
        ctx.move_to(0.0, y);
        ctx.line_to(w, y);
    }
    ctx.stroke();

    let steps = 128;
    ctx.set_stroke_style_str("#7fd1a0");
    ctx.set_line_width(2.0);
    ctx.begin_path();
    for i in 0..=steps {
        let x = i as f64 / steps as f64;
        let y = curve_value(x, shading);
        let (px, py) = (x * w, h - y * h);
        if i == 0 {
            ctx.move_to(px, py);
        } else {
            ctx.line_to(px, py);
        }
    }
    ctx.stroke();

    // Resulting tone across a slice from edge to core with vert fixed at middle.
    ctx.set_stroke_style_str("rgba(255,200,120,0.9)");
    let dash = js_sys::Array::new();
    dash.push(&4.0.into());
    dash.push(&3.0.into());
    let _ = ctx.set_line_dash(&dash);
    ctx.begin_path();
    for i in 0..=steps {
        let x = i as f64 / steps as f64;
        let t = shade_value(x, 0.5, shading);
        let (px, py) = (x * w, h - t * h);
        if i == 0 {
            ctx.move_to(px, py);
        } else {
            ctx.line_to(px, py);
        }
    }
    ctx.stroke();
    let _ = ctx.set_line_dash(&js_sys::Array::new());
}

/// Test shapes: a thick trunk slab, a round blob and a leaf ellipse, shaded by
/// exactly the same rules the sim uses.
fn build_test_mask(w: usize, h: usize) -> Vec<u8> {
    let mut mask = vec![0u8; w * h];
    let trunk_x0 = (w as f64 * 0.06).round() as usize;
    let trunk_x1 = (w as f64 * 0.2).round() as usize;
    for y in (h as f64 * 0.1).round() as usize..h - 2 {
        for x in trunk_x0..=trunk_x1 {
            mask[y * w + x] = 1;
        }
    }
    let bx = w as f64 * 0.48;
    let by = h as f64 * 0.45;
    let br = (w.min(h)) as f64 * 0.28;
    for y in 0..h {
        for x in 0..w {
            let dx = x as f64 + 0.5 - bx;
            let dy = y as f64 + 0.5 - by;
            if dx * dx + dy * dy <= br * br {
                mask[y * w + x] = 1;
            }
        }
    }
    let lx = w as f64 * 0.82;
    let ly = h as f64 * 0.5;
    let rx = w as f64 * 0.13;
    let ry = h as f64 * 0.3;
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f64 + 0.5 - lx) / rx;
            let dy = (y as f64 + 0.5 - ly) / ry;
            if dx * dx + dy * dy <= 1.0 {
                mask[y * w + x] = 1;
            }
        }
    }
    mask
}

fn draw_shapes(canvas: &HtmlCanvasElement, app: &App) {
    let (ctx, rw, rh) = match context(canvas) {
        Some(c) => c,
        None => return,
    };
    let (w, h) = (72usize, 40usize);
    let mask = build_test_mask(w, h);
    let mut dist = Vec::new();
    distance_transform(&mask, w, h, &mut dist);
    let mut labels = Vec::new();
    let mut stack = Vec::new();
    let mut comps = label_components(&mask, w, h, &mut labels, &mut stack);
    for i in 0..labels.len() {
        let l = labels[i];
        if l < 0 {
            continue;
        }
        if dist[i] > comps[l as usize].max_depth {
            comps[l as usize].max_depth = dist[i];
        }
    }

    let sampler = if app.state.materials.find(&app.ui.shade_preview_sampler).is_some() {
        app.ui.shade_preview_sampler.clone()
    } else {
        app.state.materials.samplers.first().map(|s| s.id.clone()).unwrap_or_default()
    };
    let ramp = app.state.materials.bands(&sampler);
    let core = app.ui.shade_preview_core;
    let tones = app.ui.shade_preview_tones;

    let mut bytes = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let l = labels[i];
            if l < 0 {
                continue;
            }
            let comp = comps[l as usize];
            let norm = core.min((comp.max_depth as f64).max(0.5));
            let nd = clamp01(dist[i] as f64 / norm);
            let span = comp.y1 - comp.y0;
            let vert = if span > 0 {
                (y as i32 - comp.y0) as f64 / span as f64
            } else {
                0.0
            };
            let t = quantize(shade_value(nd, vert, &app.state.shading), tones);
            // The preview reads the box the way anything else does, height and
            // all, or it would be showing something the tool never draws.
            let c = unpack_rgba(ramp.pick(t, vert));
            let o = i * 4;
            bytes[o] = c.r;
            bytes[o + 1] = c.g;
            bytes[o + 2] = c.b;
            bytes[o + 3] = 255;
        }
    }

    let off = el("canvas").get().dyn_into::<HtmlCanvasElement>().unwrap();
    off.set_width(w as u32);
    off.set_height(h as u32);
    let octx = off
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into::<CanvasRenderingContext2d>()
        .unwrap();
    if let Ok(image) = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&bytes), w as u32, h as u32)
    {
        let _ = octx.put_image_data(&image, 0.0, 0.0);
    }
    let z = (rw / w as f64).min(rh / h as f64).max(1.0);
    ctx.set_image_smoothing_enabled(false);
    let _ = ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
        &off,
        (rw - w as f64 * z) / 2.0,
        (rh - h as f64 * z) / 2.0,
        w as f64 * z,
        h as f64 * z,
    );
}
