//! The drawing half of a pixel editor, with nothing in it that knows which
//! buffer is being drawn on.
//!
//! Both editors in the tool paint the same way: a pointer walks cells, the tool
//! decides what happens at each one, and a stroke is one undoable unit that
//! ends when the pointer lifts. What differs is where the pixels live, so that
//! is the one thing a caller supplies.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Event, HtmlCanvasElement, PointerEvent};

use crate::app::{App, Handle, Tool};
use crate::ui::{on, Scope};
use crate::util::EMPTY_COLOR;

/// The buffer under an editor. Every call takes the app because what is being
/// drawn on is chosen by the panel's own selection, which lives there.
pub trait Surface: 'static {
    fn dims(&self, app: &App) -> Option<(i32, i32)>;
    fn get(&self, app: &App, x: i32, y: i32) -> u32;
    fn set(&self, app: &mut App, x: i32, y: i32, v: u32);
    /// Once per stroke, before the first cell of it is painted. The default
    /// is one step of the project's history; a surface whose buffer is not in
    /// the project keeps its own way back instead.
    fn begin(&self, app: &mut App) {
        app.record("stroke", false);
    }
    /// Once per stroke, after the pointer lifts.
    fn commit(&self, app: &mut App);
    /// Where a pointer at these client coordinates lands in the buffer. The
    /// default reads the element as though the buffer filled it exactly, which
    /// is what an editor sitting in a panel does. A surface drawn through the
    /// camera answers for itself.
    fn locate(
        &self,
        app: &App,
        canvas: &HtmlCanvasElement,
        client_x: f64,
        client_y: f64,
    ) -> Option<(i32, i32)> {
        let (w, h) = self.dims(app)?;
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
    /// What the pick tool reads. The same as `get` for a surface with one
    /// buffer; a stack of layers answers with what is on show instead, because
    /// that is the color the pointer is over.
    fn pick(&self, app: &App, x: i32, y: i32) -> u32 {
        self.get(app, x, y)
    }
}

/// Repaints the canvas from whatever the surface is showing.
pub type Draw = Rc<dyn Fn(&HtmlCanvasElement, &App)>;

fn flood_fill(app: &mut App, s: &dyn Surface, x: i32, y: i32, value: u32) {
    let (w, h) = match s.dims(app) {
        Some(d) => d,
        None => return,
    };
    let target = s.get(app, x, y);
    if target == value {
        return;
    }
    let mut stack = vec![(x, y)];
    while let Some((cx, cy)) = stack.pop() {
        if cx < 0 || cy < 0 || cx >= w || cy >= h {
            continue;
        }
        if s.get(app, cx, cy) != target {
            continue;
        }
        s.set(app, cx, cy, value);
        stack.push((cx - 1, cy));
        stack.push((cx + 1, cy));
        stack.push((cx, cy - 1));
        stack.push((cx, cy + 1));
    }
}

/// What the current tool does at one cell. Exported because the sprite editor
/// draws on the stage, where the pointer is shared with the camera and the
/// stroke is driven by whoever owns it rather than by `attach`.
pub fn apply(app: &mut App, s: &dyn Surface, cell: (i32, i32), erase: bool) {
    match app.ui.tool {
        Tool::Pick => {
            let v = s.pick(app, cell.0, cell.1);
            if v != EMPTY_COLOR {
                crate::ui::color_wheel::set_brush(app, v);
                app.redraw_panel = true;
            }
        }
        Tool::Fill => {
            let value = if erase { EMPTY_COLOR } else { app.ui.brush_color };
            flood_fill(app, s, cell.0, cell.1, value);
        }
        // The marquee is dragged out by whoever owns the pointer, not stamped
        // a cell at a time, so there is nothing to do per cell.
        Tool::Select => {}
        _ => {
            let erase = erase || app.ui.tool == Tool::Eraser;
            let value = if erase { EMPTY_COLOR } else { app.ui.brush_color };
            s.set(app, cell.0, cell.1, value);
            if app.ui.mirror_x {
                if let Some((w, _)) = s.dims(app) {
                    s.set(app, w - 1 - cell.0, cell.1, value);
                }
            }
        }
    }
}

/// The cells between two pointer positions, so a fast drag draws a line rather
/// than a dotted one.
pub fn stroke_line(app: &mut App, s: &dyn Surface, a: (i32, i32), b: (i32, i32), erase: bool) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs());
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let x = (a.0 as f64 + (b.0 - a.0) as f64 * t).round() as i32;
        let y = (a.1 as f64 + (b.1 - a.1) as f64 * t).round() as i32;
        apply(app, s, (x, y), erase);
    }
}

/// Binds a canvas to a surface. The canvas keeps drawing itself through `draw`,
/// which is also what the owning panel calls on a redraw.
pub fn attach(canvas: &HtmlCanvasElement, h: &Handle, surface: Rc<dyn Surface>, draw: Draw) {
    let last: Rc<RefCell<Option<(i32, i32)>>> = Rc::new(RefCell::new(None));
    let drawing = Rc::new(RefCell::new(false));

    {
        let h2 = h.clone();
        let canvas2 = canvas.clone();
        let last = last.clone();
        let drawing = drawing.clone();
        let surface = surface.clone();
        let draw = draw.clone();
        on(canvas.unchecked_ref(), "pointerdown", Scope::Panel, move |e: Event| {
            let pe = match e.dyn_ref::<PointerEvent>() {
                Some(pe) => pe,
                None => return,
            };
            let _ = canvas2.set_pointer_capture(pe.pointer_id());
            let mut sh = h2.borrow_mut();
            let cell = surface.locate(
                &sh.app,
                &canvas2,
                pe.client_x() as f64,
                pe.client_y() as f64,
            );
            *drawing.borrow_mut() = true;
            *last.borrow_mut() = cell;
            if let Some(cell) = cell {
                // A stroke is one step back however many cells the pointer
                // walks over. The pick tool reads and does not write, so it has
                // nothing to put back and should not push real edits off the
                // stack.
                if sh.app.ui.tool != Tool::Pick {
                    surface.begin(&mut sh.app);
                }
                let erase = pe.buttons() & 2 == 2;
                apply(&mut sh.app, surface.as_ref(), cell, erase);
                draw(&canvas2, &sh.app);
            }
        });
    }
    {
        let h2 = h.clone();
        let canvas2 = canvas.clone();
        let last = last.clone();
        let drawing = drawing.clone();
        let surface = surface.clone();
        let draw = draw.clone();
        on(canvas.unchecked_ref(), "pointermove", Scope::Panel, move |e: Event| {
            if !*drawing.borrow() {
                return;
            }
            let pe = match e.dyn_ref::<PointerEvent>() {
                Some(pe) => pe,
                None => return,
            };
            let mut sh = h2.borrow_mut();
            let cell = match surface.locate(
                &sh.app,
                &canvas2,
                pe.client_x() as f64,
                pe.client_y() as f64,
            ) {
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
                Some(prev) if freehand => {
                    stroke_line(&mut sh.app, surface.as_ref(), prev, cell, erase)
                }
                _ => apply(&mut sh.app, surface.as_ref(), cell, erase),
            }
            *last.borrow_mut() = Some(cell);
            draw(&canvas2, &sh.app);
        });
    }
    for event in ["pointerup", "pointercancel", "pointerleave"] {
        let h2 = h.clone();
        let last = last.clone();
        let drawing = drawing.clone();
        let surface = surface.clone();
        on(canvas.unchecked_ref(), event, Scope::Panel, move |_| {
            if !*drawing.borrow() {
                return;
            }
            *drawing.borrow_mut() = false;
            *last.borrow_mut() = None;
            let mut sh = h2.borrow_mut();
            surface.commit(&mut sh.app);
            sh.app.redraw_panel = true;
        });
    }
    {
        // A right button drag erases, and the menu it would otherwise open
        // would land in the middle of the stroke.
        on(canvas.unchecked_ref(), "contextmenu", Scope::Panel, move |e: Event| {
            e.prevent_default();
        });
    }
}

/// The hairlines between cells, drawn only while the cells are large enough for
/// them to read as a grid rather than as a screen door.
pub fn cell_grid(
    ctx: &CanvasRenderingContext2d,
    rw: f64,
    rh: f64,
    gw: i32,
    gh: i32,
    cw: f64,
    ch: f64,
) {
    if cw.min(ch) < 7.0 {
        return;
    }
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
