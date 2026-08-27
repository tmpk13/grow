//! An HSV color wheel for the brush: hue around, saturation out from the
//! middle, and a slider for value.
//!
//! The brush keeps its hue and saturation rather than being read back out of
//! the packed color every time, because black and white have no hue to
//! recover and the wheel would jump to red the moment the value slider reached
//! either end.
//!
//! The disc is a raster the widget builds itself, at a resolution of its own
//! rather than the element's, so the stylesheet is free to size it in whatever
//! units the layout calls for. Rebuilding it costs a walk over that raster, so
//! it is kept until the value moves - in a canvas of its own rather than as an
//! `ImageData`, because an `ImageData` built from wasm memory is a view onto
//! that memory rather than a copy of it, and keeping one past the life of the
//! buffer behind it means drawing whatever was allocated there since.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::{Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, Element, Event, HtmlCanvasElement, HtmlInputElement, ImageData,
    PointerEvent,
};

use crate::app::{App, Handle};
use crate::ui::{el, input_el, on, row, Scope, Tap};
use crate::util::{hsv_to_packed, packed_to_hex};

/// Pixels across the disc raster. Two of these fit an ordinary panel column at
/// any font size the tool runs at, and the disc is drawn scaled to whatever the
/// stylesheet gives it.
const DISC_PX: i32 = 176;

pub struct ColorWheel {
    pub root: Element,
    canvas: HtmlCanvasElement,
    value: HtmlInputElement,
    hex: HtmlInputElement,
    /// The disc as last rastered. A canvas rather than an `ImageData`: these
    /// pixels belong to the browser and outlive the buffer they came from.
    disc: HtmlCanvasElement,
    /// The value the disc was rastered for, in hundredths.
    at: Cell<i32>,
}

/// Sets the brush from a wheel position, keeping hue and saturation apart from
/// the packed color so the two ends of the value slider stay on the same hue.
fn set_hsv(app: &mut App, h: f64, s: f64, v: f64) {
    app.ui.brush_hsv = (h, s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    app.ui.brush_color = hsv_to_packed(h, s, v);
}

impl ColorWheel {
    pub fn attach(h: &Handle, app: &App) -> Rc<ColorWheel> {
        let val = app.ui.brush_hsv.2;

        let canvas = el("canvas")
            .class("wheel-canvas")
            .get()
            .dyn_into::<HtmlCanvasElement>()
            .unwrap();
        canvas.set_width(DISC_PX as u32);
        canvas.set_height(DISC_PX as u32);

        let value = input_el("range").tap(|i| {
            let _ = i.set_attribute("min", "0");
            let _ = i.set_attribute("max", "1");
            let _ = i.set_attribute("step", "0.01");
            i.set_value(&format!("{val}"));
        });

        let hex = input_el("text").tap(|i| {
            i.set_class_name("hex");
            let _ = i.set_attribute("spellcheck", "false");
            i.set_value(&packed_to_hex(app.ui.brush_color));
        });

        let root = el("div")
            .class("wheel")
            .child(canvas.unchecked_ref())
            .child(&row("Value", value.clone().unchecked_into(), None))
            .child(&row("Hex", hex.clone().unchecked_into(), None))
            .get();

        let disc = el("canvas")
            .get()
            .dyn_into::<HtmlCanvasElement>()
            .unwrap();
        disc.set_width(DISC_PX as u32);
        disc.set_height(DISC_PX as u32);

        let wheel = Rc::new(ColorWheel {
            root,
            canvas,
            value,
            hex,
            disc,
            at: Cell::new(i32::MIN),
        });

        // Ignore hue and saturation while the pointer is off the disc, so a
        // drag that runs past the edge holds the last color instead of
        // snapping to full saturation.
        let dragging = Rc::new(RefCell::new(false));
        for event in ["pointerdown", "pointermove"] {
            let h2 = h.clone();
            let wheel2 = wheel.clone();
            let dragging = dragging.clone();
            on(wheel.canvas.unchecked_ref(), event, Scope::Panel, move |e: Event| {
                let pe = match e.dyn_ref::<PointerEvent>() {
                    Some(pe) => pe,
                    None => return,
                };
                if e.type_() == "pointerdown" {
                    *dragging.borrow_mut() = true;
                    let _ = wheel2.canvas.set_pointer_capture(pe.pointer_id());
                } else if !*dragging.borrow() {
                    return;
                }
                let r = wheel2.canvas.get_bounding_client_rect();
                if r.width() <= 0.0 || r.height() <= 0.0 {
                    return;
                }
                let nx = (pe.client_x() as f64 - r.left()) / r.width() * 2.0 - 1.0;
                let ny = (pe.client_y() as f64 - r.top()) / r.height() * 2.0 - 1.0;
                let dist = (nx * nx + ny * ny).sqrt();
                if dist > 1.0 {
                    return;
                }
                let mut sh = h2.borrow_mut();
                let v = sh.app.ui.brush_hsv.2;
                let hue = ny.atan2(nx).to_degrees() + 90.0;
                set_hsv(&mut sh.app, hue, dist, v);
                wheel2.sync(&sh.app);
                sh.app.redraw_panel = true;
            });
        }
        for event in ["pointerup", "pointercancel", "pointerleave"] {
            let dragging = dragging.clone();
            on(wheel.canvas.unchecked_ref(), event, Scope::Panel, move |_| {
                *dragging.borrow_mut() = false;
            });
        }

        {
            let h2 = h.clone();
            let wheel2 = wheel.clone();
            on(wheel.value.unchecked_ref(), "input", Scope::Panel, move |e| {
                let v: f64 = crate::ui::value_of(&e).parse().unwrap_or(1.0);
                let mut sh = h2.borrow_mut();
                let (hue, sat, _) = sh.app.ui.brush_hsv;
                set_hsv(&mut sh.app, hue, sat, v);
                wheel2.draw(&sh.app);
                wheel2.sync(&sh.app);
                sh.app.redraw_panel = true;
            });
        }
        {
            // Typed hex is the one way in that does not come from the wheel, so
            // it is also the one place hue and saturation are read back out of
            // a color.
            let h2 = h.clone();
            let wheel2 = wheel.clone();
            on(wheel.hex.unchecked_ref(), "change", Scope::Panel, move |e| {
                let text = crate::ui::value_of(&e);
                let packed = crate::util::hex_to_packed(&text);
                let mut sh = h2.borrow_mut();
                let keep = sh.app.ui.brush_hsv.0;
                let (hue, sat, val) = crate::util::packed_to_hsv(packed, keep);
                set_hsv(&mut sh.app, hue, sat, val);
                wheel2.draw(&sh.app);
                wheel2.sync(&sh.app);
                sh.app.redraw_panel = true;
            });
        }

        wheel.draw(app);
        wheel
    }

    /// Puts the wheel back in step with a brush color that was set elsewhere:
    /// by the pick tool, a swatch, or the plain color box next to it.
    pub fn sync(&self, app: &App) {
        self.value.set_value(&format!("{}", app.ui.brush_hsv.2));
        let hex = packed_to_hex(app.ui.brush_color);
        if self.hex.value() != hex {
            self.hex.set_value(&hex);
        }
        self.mark(app);
    }

    /// Rasters the disc for the current value and marks where the brush sits.
    pub fn draw(&self, app: &App) {
        let key = (app.ui.brush_hsv.2 * 100.0).round() as i32;
        if self.at.get() != key {
            if let (Some(ctx), Some(image)) = (context(&self.disc), raster(app.ui.brush_hsv.2)) {
                // The image is put while the buffer behind it is still alive,
                // and what is kept afterwards is the canvas it landed in.
                ctx.clear_rect(0.0, 0.0, DISC_PX as f64, DISC_PX as f64);
                let _ = ctx.put_image_data(&image, 0.0, 0.0);
                self.at.set(key);
            }
        }
        self.mark(app);
    }

    fn mark(&self, app: &App) {
        let ctx = match context(&self.canvas) {
            Some(c) => c,
            None => return,
        };
        ctx.clear_rect(0.0, 0.0, DISC_PX as f64, DISC_PX as f64);
        let _ = ctx.draw_image_with_html_canvas_element(&self.disc, 0.0, 0.0);
        let (hue, sat, _) = app.ui.brush_hsv;
        let radius = DISC_PX as f64 / 2.0;
        let angle = (hue - 90.0).to_radians();
        let x = radius + angle.cos() * sat * (radius - 1.0);
        let y = radius + angle.sin() * sat * (radius - 1.0);
        ctx.begin_path();
        let _ = ctx.arc(x, y, radius * 0.05, 0.0, std::f64::consts::TAU);
        ctx.set_line_width(2.0);
        ctx.set_stroke_style_str("#0a0d12");
        ctx.stroke();
        ctx.begin_path();
        let _ = ctx.arc(x, y, radius * 0.05, 0.0, std::f64::consts::TAU);
        ctx.set_line_width(1.0);
        ctx.set_stroke_style_str("#ffffff");
        ctx.stroke();
    }
}

fn context(canvas: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()
}

/// The disc at one value. Outside the circle is left transparent, and the last
/// pixel of the radius is faded rather than cut, so the rim does not read as a
/// staircase.
fn raster(value: f64) -> Option<ImageData> {
    let n = DISC_PX;
    let radius = n as f64 / 2.0;
    let mut bytes = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            let dx = (x as f64 + 0.5 - radius) / radius;
            let dy = (y as f64 + 0.5 - radius) / radius;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > 1.0 {
                continue;
            }
            let hue = dy.atan2(dx).to_degrees() + 90.0;
            let c = crate::util::unpack_rgba(hsv_to_packed(hue, dist, value));
            let edge = ((1.0 - dist) * radius).clamp(0.0, 1.0);
            let o = ((y * n + x) * 4) as usize;
            bytes[o] = c.r;
            bytes[o + 1] = c.g;
            bytes[o + 2] = c.b;
            bytes[o + 3] = (edge * 255.0).round() as u8;
        }
    }
    ImageData::new_with_u8_clamped_array_and_sh(Clamped(&bytes), n as u32, n as u32).ok()
}

/// The brush color control: the plain color box, and the wheel behind a
/// switch, because a color box is quicker for a color somebody already knows
/// and the wheel is for finding one.
pub struct Brush {
    pub rows: Vec<Element>,
    color_input: HtmlInputElement,
    wheel: Option<Rc<ColorWheel>>,
}

impl Brush {
    pub fn build(h: &Handle, app: &App) -> Brush {
        let color_input = input_el("color");
        color_input.set_value(&packed_to_hex(app.ui.brush_color));
        {
            let h2 = h.clone();
            on(color_input.unchecked_ref(), "input", Scope::Panel, move |e| {
                let mut sh = h2.borrow_mut();
                let packed = crate::util::hex_to_packed(&crate::ui::value_of(&e));
                let keep = sh.app.ui.brush_hsv.0;
                let (hue, sat, val) = crate::util::packed_to_hsv(packed, keep);
                set_hsv(&mut sh.app, hue, sat, val);
                sh.app.redraw_panel = true;
            });
        }

        let toggle = {
            let h2 = h.clone();
            let class = if app.ui.use_wheel { "btn active" } else { "btn" };
            el("button")
                .class(class)
                .attr("type", "button")
                .text("Wheel")
                .on("click", Scope::Panel, move |_| {
                    let mut sh = h2.borrow_mut();
                    sh.app.ui.use_wheel = !sh.app.ui.use_wheel;
                    sh.app.rebuild_panel();
                })
                .get()
        };

        let mut rows = vec![row(
            "Brush color",
            el("span")
                .class("inline")
                .child(color_input.unchecked_ref())
                .child(&toggle)
                .get(),
            Some("the wheel is hue around, saturation out from the middle"),
        )];
        let wheel = if app.ui.use_wheel {
            let built = ColorWheel::attach(h, app);
            rows.push(built.root.clone());
            Some(built)
        } else {
            None
        };
        Brush { rows, color_input, wheel }
    }

    /// Puts both controls back in step with a brush that was changed elsewhere:
    /// by the pick tool, or by a swatch.
    pub fn sync(&self, app: &App) {
        self.color_input.set_value(&packed_to_hex(app.ui.brush_color));
        if let Some(wheel) = &self.wheel {
            wheel.draw(app);
            wheel.sync(app);
        }
    }
}

/// Sets the brush from a color that came from somewhere with no hue of its
/// own to offer, such as a swatch or the pick tool.
pub fn set_brush(app: &mut App, packed: u32) {
    let keep = app.ui.brush_hsv.0;
    let (hue, sat, val) = crate::util::packed_to_hsv(packed, keep);
    app.ui.brush_hsv = (hue, sat, val);
    app.ui.brush_color = packed;
}
