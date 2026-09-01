//! Drawing the map from a picture.
//!
//! A dropped image is laid over the whole map as a landscape: press on it to
//! take the color under the pointer and drag out the piece of it to work on,
//! and every cell in that box whose color is near enough to the one taken is
//! turned into whatever is chosen - water, rock, grass or sand, or a zone
//! saying what may take root there.
//!
//! The picture is never part of the settlement. What it leaves behind is cells,
//! which is what the map is made of; the image itself is a tool, kept for as
//! long as the page is open and no longer.

use wasm_bindgen::{Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, DragEvent, Element, Event, HtmlCanvasElement, ImageData,
};

use crate::app::{App, Handle};
use crate::civ::terrain::Cell;
use crate::ui::{
    app_num, button, document, el, input_el, note, on, section, select_field, NumOpts, Scope, Tap,
};
use crate::util::unpack_rgba;
use crate::world::Zone;

/// What a matching cell is turned into. The ground and what grows on it are
/// two different questions about the same cell, so they are one list of
/// answers rather than two menus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Make {
    Ground(Cell),
    Growth(Zone),
}

impl Default for Make {
    /// Water: the one thing a picture of a landscape is most often dropped in
    /// to draw, and the only one of them that changes where people can walk.
    fn default() -> Self {
        Make::Ground(Cell::Water)
    }
}

impl Make {
    pub fn key(self) -> &'static str {
        match self {
            Make::Ground(Cell::Grass) => "grass",
            Make::Ground(Cell::Water) => "water",
            Make::Ground(Cell::Rock) => "rock",
            Make::Ground(Cell::Sand) => "sand",
            Make::Ground(Cell::Cliff) => "cliff",
            Make::Growth(Zone::Any) => "anything",
            Make::Growth(Zone::Bare) => "bare",
            Make::Growth(Zone::Wood) => "wood",
            Make::Growth(Zone::Low) => "low",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Make::Ground(Cell::Grass) => "Ground: grass",
            Make::Ground(Cell::Water) => "Ground: water",
            Make::Ground(Cell::Rock) => "Ground: rock",
            Make::Ground(Cell::Sand) => "Ground: sand",
            Make::Ground(Cell::Cliff) => "Ground: rock face (nobody crosses)",
            Make::Growth(Zone::Any) => "Growth: anything",
            Make::Growth(Zone::Bare) => "Growth: nothing",
            Make::Growth(Zone::Wood) => "Growth: trees only",
            Make::Growth(Zone::Low) => "Growth: low only",
        }
    }

    pub fn from_key(key: &str) -> Make {
        MAKES.iter().copied().find(|m| m.key() == key).unwrap_or(Make::Ground(Cell::Water))
    }
}

pub const MAKES: [Make; 9] = [
    Make::Ground(Cell::Water),
    Make::Ground(Cell::Rock),
    Make::Ground(Cell::Cliff),
    Make::Ground(Cell::Grass),
    Make::Ground(Cell::Sand),
    Make::Growth(Zone::Bare),
    Make::Growth(Zone::Wood),
    Make::Growth(Zone::Low),
    Make::Growth(Zone::Any),
];

/// The image, the box dragged on it, and the color taken from it. All of it is
/// how somebody is using the map rather than anything about the map, so none of
/// it is saved.
#[derive(Clone, Default)]
pub struct Landscape {
    pub image: Option<(i32, i32, Vec<u32>)>,
    pub name: String,
    /// The corners of the drag, in cells, in the order they were dragged.
    pub box_cells: Option<(i32, i32, i32, i32)>,
    /// The color the drag started on, which is what everything in the box is
    /// measured against.
    pub color: u32,
    pub picked: bool,
    /// How far from that color still counts, as a fraction of the furthest two
    /// colors can be.
    pub threshold: f64,
    pub make: Make,
}

impl Landscape {
    /// The box, in cells, lowest corner first, clamped to the map.
    pub fn rect(&self, cols: i32, rows: i32) -> Option<(i32, i32, i32, i32)> {
        let (a, b, c, d) = self.box_cells?;
        let (c0, c1) = (a.min(c).max(0), a.max(c).min(cols - 1));
        let (r0, r1) = (b.min(d).max(0), b.max(d).min(rows - 1));
        if c0 > c1 || r0 > r1 {
            return None;
        }
        Some((c0, r0, c1, r1))
    }

    /// The color the picture has over a cell. The image is stretched over the
    /// whole map, which is what "used as a landscape" means: the corners are
    /// the corners.
    pub fn at(&self, cols: i32, rows: i32, col: i32, row: i32) -> Option<u32> {
        let (w, h, px) = self.image.as_ref()?;
        let x = ((col as f64 + 0.5) / cols.max(1) as f64 * *w as f64).floor() as i32;
        let y = ((row as f64 + 0.5) / rows.max(1) as f64 * *h as f64).floor() as i32;
        let x = x.clamp(0, w - 1);
        let y = y.clamp(0, h - 1);
        px.get((y * w + x) as usize).copied()
    }

    /// Every cell in the box whose color is near enough to the one taken.
    pub fn matches(&self, cols: i32, rows: i32) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        let (c0, r0, c1, r1) = match self.rect(cols, rows) {
            Some(rect) => rect,
            None => return out,
        };
        for r in r0..=r1 {
            for c in c0..=c1 {
                match self.at(cols, rows, c, r) {
                    Some(v) if near(v, self.color, self.threshold) => out.push((c, r)),
                    _ => {}
                }
            }
        }
        out
    }
}

/// Whether two colors are within the threshold of each other, as a fraction of
/// the furthest apart two colors can be.
pub fn near(a: u32, b: u32, threshold: f64) -> bool {
    let (a, b) = (unpack_rgba(a), unpack_rgba(b));
    let d = ((a.r as f64 - b.r as f64).powi(2)
        + (a.g as f64 - b.g as f64).powi(2)
        + (a.b as f64 - b.b as f64).powi(2))
    .sqrt();
    d / (255.0 * 3.0f64.sqrt()) <= threshold.clamp(0.0, 1.0)
}

pub fn build(app: &App, h: &Handle) -> Element {
    let land = app.ui.land.clone();
    let (cols, rows) = match app.settlement.as_ref() {
        Some(sim) => (sim.world().cols, sim.world().rows),
        None => (0, 0),
    };
    let readout = el("span").class("field-hint").attr("id", "zone-readout").get();
    let canvas = preview(app, h, &readout);

    let mut rows_ui = vec![
        note(
            "Drop a picture of the land and it is laid over the whole map, corner to corner. \
             Press on it to take the color under the pointer and drag out the piece to work on; \
             every cell in that box whose color is near enough to the one taken becomes whatever \
             is chosen below. Ground is what the cell is made of; growth is what may take root \
             there, which leaves the ground alone and holds for whatever grows next.",
        ),
        drop_zone(h),
    ];
    if let Some(canvas) = canvas {
        rows_ui.push(canvas);
    }
    let makes: Vec<(String, String)> =
        MAKES.iter().map(|m| (m.key().to_string(), m.label().to_string())).collect();
    rows_ui.push(select_field("Make it", land.make.key(), &makes, None, {
        let h2 = h.clone();
        move |v| {
            h2.borrow_mut().app.ui.land.make = Make::from_key(&v);
        }
    }));
    rows_ui.push(app_num(
        h,
        "How near the color",
        land.threshold,
        NumOpts { min: 0.0, max: 1.0, step: 0.01 },
        Some("0 takes only that exact color; 1 takes the whole box whatever it looks like"),
        |app, v| app.ui.land.threshold = v,
    ));
    rows_ui.push(readout.clone());

    let h2 = h.clone();
    rows_ui.push(button("Apply to the map", Scope::Panel, move || {
        let mut sh = h2.borrow_mut();
        let sh = &mut *sh;
        let land = sh.app.ui.land.clone();
        let said = match sh.app.settlement.as_mut() {
            Some(sim) => {
                let (cols, rows) = (sim.world().cols, sim.world().rows);
                let cells = land.matches(cols, rows);
                if cells.is_empty() {
                    "nothing in the box is near that color".to_string()
                } else {
                    let done = match land.make {
                        Make::Ground(kind) => sim.paint_cells(&cells, kind),
                        Make::Growth(zone) => sim.zone_cells(&cells, zone),
                    };
                    let refused = cells.len() - done;
                    let what = land.make.label().to_lowercase();
                    if refused > 0 {
                        format!("{done} cells are {what}; {refused} were built on and left")
                    } else {
                        format!("{done} cells are {what}")
                    }
                }
            }
            None => "no map to draw on".to_string(),
        };
        sh.app.set_note(&said);
        sh.app.civ_stepped = true;
    }));

    if land.image.is_some() {
        let h2 = h.clone();
        rows_ui.push(crate::ui::danger_button("Forget the picture", Scope::Panel, move || {
            let mut sh = h2.borrow_mut();
            sh.app.ui.land = Landscape { threshold: sh.app.ui.land.threshold, ..Landscape::default() };
            sh.app.rebuild_panel = true;
        }));
    }
    let _ = (cols, rows);
    section("Zones from a picture", rows_ui)
}

/// The picture, drawn at whatever width the panel gives it, with the box over
/// it. Redrawn in place rather than by rebuilding the panel, because it follows
/// a drag.
fn preview(app: &App, h: &Handle, readout: &Element) -> Option<Element> {
    let (w, h_px, _) = app.ui.land.image.as_ref()?;
    let canvas = document()
        .create_element("canvas")
        .ok()?
        .dyn_into::<HtmlCanvasElement>()
        .ok()?;
    canvas.set_class_name("landscape");
    canvas.set_width(*w as u32);
    canvas.set_height(*h_px as u32);
    let node: Element = canvas.clone().unchecked_into();
    draw(app, &canvas);

    for event in ["pointerdown", "pointermove", "pointerup"] {
        let h2 = h.clone();
        let canvas2 = canvas.clone();
        let readout = readout.clone();
        on(node.unchecked_ref(), event, Scope::Panel, move |e: Event| {
            let pe = match e.dyn_ref::<web_sys::PointerEvent>() {
                Some(pe) => pe,
                None => return,
            };
            if event != "pointerdown" && pe.buttons() == 0 {
                return;
            }
            e.prevent_default();
            let mut sh = h2.borrow_mut();
            let sh = &mut *sh;
            let (cols, rows) = match sh.app.settlement.as_ref() {
                Some(sim) => (sim.world().cols, sim.world().rows),
                None => return,
            };
            let rect = canvas2.get_bounding_client_rect();
            let fx = ((pe.client_x() as f64 - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
            let fy = ((pe.client_y() as f64 - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
            let col = ((fx * cols as f64).floor() as i32).clamp(0, cols - 1);
            let row = ((fy * rows as f64).floor() as i32).clamp(0, rows - 1);
            if event == "pointerdown" {
                let _ = canvas2.set_pointer_capture(pe.pointer_id());
                sh.app.ui.land.color =
                    sh.app.ui.land.at(cols, rows, col, row).unwrap_or(0xff00_ffff);
                sh.app.ui.land.picked = true;
                sh.app.ui.land.box_cells = Some((col, row, col, row));
            } else if let Some(b) = &mut sh.app.ui.land.box_cells {
                b.2 = col;
                b.3 = row;
            }
            draw(&sh.app, &canvas2);
            let n = sh.app.ui.land.matches(cols, rows).len();
            let (c0, r0, c1, r1) = sh.app.ui.land.rect(cols, rows).unwrap_or((0, 0, 0, 0));
            readout.set_text_content(Some(&format!(
                "{n} cells match, of {} in the box ({}x{} cells from {c0},{r0})",
                (c1 - c0 + 1) * (r1 - r0 + 1),
                c1 - c0 + 1,
                r1 - r0 + 1
            )));
        });
    }
    Some(node)
}

/// The picture and the box over it, straight into the canvas.
fn draw(app: &App, canvas: &HtmlCanvasElement) {
    let (w, h, px) = match app.ui.land.image.as_ref() {
        Some(img) => img,
        None => return,
    };
    let ctx = match canvas.get_context("2d").ok().flatten() {
        Some(c) => match c.dyn_into::<CanvasRenderingContext2d>() {
            Ok(c) => c,
            Err(_) => return,
        },
        None => return,
    };
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for v in px {
        let c = unpack_rgba(*v);
        bytes.extend_from_slice(&[c.r, c.g, c.b, 255]);
    }
    if let Ok(image) =
        ImageData::new_with_u8_clamped_array_and_sh(Clamped(&bytes), *w as u32, *h as u32)
    {
        let _ = ctx.put_image_data(&image, 0.0, 0.0);
    }
    let (cols, rows) = match app.settlement.as_ref() {
        Some(sim) => (sim.world().cols, sim.world().rows),
        None => return,
    };
    if let Some((c0, r0, c1, r1)) = app.ui.land.rect(cols, rows) {
        let x = c0 as f64 / cols as f64 * *w as f64;
        let y = r0 as f64 / rows as f64 * *h as f64;
        let bw = (c1 - c0 + 1) as f64 / cols as f64 * *w as f64;
        let bh = (r1 - r0 + 1) as f64 / rows as f64 * *h as f64;
        ctx.set_line_width((*w as f64 / 200.0).max(1.0));
        ctx.set_stroke_style_str("#ffffff");
        ctx.stroke_rect(x, y, bw, bh);
        ctx.set_stroke_style_str("#000000");
        ctx.stroke_rect(x + 1.0, y + 1.0, bw - 2.0, bh - 2.0);
    }
}

fn drop_zone(h: &Handle) -> Element {
    let picker = input_el("file").tap(|i| {
        i.set_accept("image/*");
        let _ = i.set_attribute("hidden", "hidden");
    });
    let zone = el("div").class("dropzone").get();
    let _ = zone.append_child(
        &el("span").class("dropzone-hint").text("Drop a picture of the land").get(),
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
                sh.app.ui.land.image = Some((w, height, px));
                sh.app.ui.land.name = name.clone();
                sh.app.ui.land.box_cells = None;
                sh.app.set_note(&format!("{name} laid over the map"));
            }
            None => sh.app.set_note("nothing readable in that drop"),
        }
        sh.app.rebuild_panel = true;
    });
}
