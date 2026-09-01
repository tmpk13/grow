//! Canvas presentation: blits a world pixel buffer through a zoomable,
//! pannable camera and draws the debug overlays on top.

use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use crate::civ::civ_render::{building_labels, colony_labels, harvest_marks, lamp_lights};
use crate::civ::settlement::{Rect, Settlement};
use crate::plant::Plant;
use crate::species::LAYER_COUNT;
use crate::state::State;
use crate::ui::{document, window};
use crate::util::{clamp, hex_to_packed, unpack_rgba};
use crate::world::World;

/// A sample never stands for more than this many world pixels each way: past
/// it the map is a thumbnail and the saving is already spent.
pub const MAX_SAMPLE_STEP: i32 = 8;

fn round_up(v: i32, step: i32) -> i32 {
    if step <= 1 {
        v
    } else {
        ((v + step - 1) / step) * step
    }
}

/// How many cuttable plants the pulse is drawn round at once. A view of a
/// forest is thousands; past a few hundred outlines the screen is a haze and
/// the rest cost frames for nothing.
const HARVEST_MARK_LIMIT: usize = 400;

/// Seconds for one breath of the pulse.
const PULSE_SECONDS: f64 = 2.4;

const LAYER_COLORS: [&str; 5] = [
    "rgba(120, 220, 140, 0.30)",
    "rgba(230, 220, 110, 0.30)",
    "rgba(120, 190, 240, 0.30)",
    "rgba(240, 140, 120, 0.30)",
    "rgba(200, 130, 240, 0.30)",
];

pub struct Viewport {
    pub canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    off: HtmlCanvasElement,
    off_ctx: CanvasRenderingContext2d,
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub dpr: f64,
    pub show_grid: bool,
    pub show_occupancy: bool,
    /// The canvas size as of the last resize, so a canvas that changes size
    /// can keep what was in the middle of the view in the middle of it.
    last_size: (f64, f64),
    /// One visible region, repacked for upload. Kept here so a frame does not
    /// allocate.
    scratch: Vec<u32>,
    /// The cloud tile as a canvas the context can repeat, and the key of the
    /// tile it holds so an unchanged frame skips the upload.
    clouds_tile: HtmlCanvasElement,
    clouds_tile_ctx: CanvasRenderingContext2d,
    clouds_key: u64,
    /// Set for the frames whose empty space is sky; the world is drawn over
    /// its own rectangle afterwards.
    space_clouds: Option<SpaceClouds>,
}

/// What the letterbox needs to be sky: where the horizon gradient sits and
/// how far the clouds have drifted.
struct SpaceClouds {
    drift: i32,
    sky_px: i32,
    /// The world row the clouds start on, the same line the map's own sky band
    /// uses, so the weather does not step at the edge of the map.
    cloud_top: i32,
    top: String,
    bottom: String,
}

fn context_of(canvas: &HtmlCanvasElement) -> CanvasRenderingContext2d {
    canvas
        .get_context("2d")
        .expect("2d context")
        .expect("2d context")
        .dyn_into::<CanvasRenderingContext2d>()
        .expect("2d context")
}

fn new_canvas() -> HtmlCanvasElement {
    document()
        .create_element("canvas")
        .unwrap()
        .dyn_into::<HtmlCanvasElement>()
        .unwrap()
}

/// Pushes a packed pixel buffer into a canvas of the same size.
fn put_buffer(ctx: &CanvasRenderingContext2d, buf: &[u32], w: i32, h: i32) {
    // The buffer is already laid out as RGBA bytes for a little endian machine,
    // which is what ImageData expects.
    //
    // Wrapping the wasm heap in a view instead, to save the copy, is far
    // slower: an ImageData backed by the whole heap buffer misses whatever
    // fast path the canvas has for one it owns, and the upload goes from
    // about a millisecond to a hundred.
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4) };
    if let Ok(image) =
        ImageData::new_with_u8_clamped_array_and_sh(Clamped(bytes), w as u32, h as u32)
    {
        let _ = ctx.put_image_data(&image, 0.0, 0.0);
    }
}

impl Viewport {
    pub fn new(canvas: HtmlCanvasElement) -> Self {
        let ctx = context_of(&canvas);
        let off = new_canvas();
        let off_ctx = context_of(&off);
        let clouds_tile = new_canvas();
        let clouds_tile_ctx = context_of(&clouds_tile);
        Viewport {
            canvas,
            ctx,
            off,
            off_ctx,
            clouds_tile,
            clouds_tile_ctx,
            clouds_key: 0,
            space_clouds: None,
            zoom: 2.0,
            pan_x: 0.0,
            pan_y: 0.0,
            dpr: 1.0,
            show_grid: false,
            show_occupancy: false,
            last_size: (0.0, 0.0),
            scratch: Vec::new(),
        }
    }

    fn rect(&self) -> (f64, f64) {
        let r = self.canvas.get_bounding_client_rect();
        (r.width(), r.height())
    }

    pub fn resize(&mut self) {
        let (rw, rh) = self.rect();
        self.dpr = window().device_pixel_ratio();
        let w = ((rw * self.dpr).round() as u32).max(1);
        let h = ((rh * self.dpr).round() as u32).max(1);
        if self.canvas.width() != w || self.canvas.height() != h {
            self.canvas.set_width(w);
            self.canvas.set_height(h);
        }
        // Room appearing on one side of the canvas is not a reason for the
        // world to slide to the other. Panning by half of whatever the canvas
        // gained or lost keeps the middle of the view where it was, which is
        // what a window resize, the menu folding away and going fullscreen all
        // want. The first measurement has nothing to compare against.
        let (was_w, was_h) = self.last_size;
        let measured = rw > 0.0 && rh > 0.0 && was_w > 0.0 && was_h > 0.0;
        if measured && (rw != was_w || rh != was_h) {
            self.pan_x += (rw - was_w) / 2.0;
            self.pan_y += (rh - was_h) / 2.0;
        }
        if rw > 0.0 && rh > 0.0 {
            self.last_size = (rw, rh);
        }
    }

    /// The zoom a world would be framed at, without framing it. Kept apart from
    /// `fit` so a caller can ask whether the view is already at least that
    /// wide before deciding to move the camera.
    pub fn fit_zoom(&self, world: &World) -> f64 {
        let (rw, rh) = self.rect();
        if rw == 0.0 || rh == 0.0 {
            return self.zoom;
        }
        let zx = rw / world.px_w as f64;
        let zy = rh / world.px_h as f64;
        clamp(zx.min(zy), 0.25, 24.0)
    }

    pub fn fit(&mut self, world: &World) {
        let (rw, rh) = self.rect();
        if rw == 0.0 || rh == 0.0 {
            return;
        }
        self.zoom = self.fit_zoom(world);
        self.pan_x = (rw - world.px_w as f64 * self.zoom) / 2.0;
        self.pan_y = (rh - world.px_h as f64 * self.zoom) / 2.0;
    }

    pub fn zoom_at(&mut self, client_x: f64, client_y: f64, factor: f64) {
        let r = self.canvas.get_bounding_client_rect();
        let cx = client_x - r.left();
        let cy = client_y - r.top();
        let next = clamp(self.zoom * factor, 0.25, 32.0);
        let k = next / self.zoom;
        self.pan_x = cx - (cx - self.pan_x) * k;
        self.pan_y = cy - (cy - self.pan_y) * k;
        self.zoom = next;
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.pan_x += dx;
        self.pan_y += dy;
    }

    /// How many world pixels one uploaded pixel stands for.
    ///
    /// Zoomed out past 1:1 the canvas throws away all but one pixel of every
    /// block it draws, so producing the rest is work nobody sees. The region is
    /// sampled on a grid of this step instead, and the step is a whole number
    /// aligned to the origin rather than to the view, so panning slides the
    /// image without changing which pixels survive.
    pub fn sample_step(&self) -> i32 {
        // The live ratio rather than the cached one: the settlement asks for
        // the step before the frame resizes the canvas, and a step that
        // disagreed with the upload's would leave it reading rows the
        // compositing had skipped.
        let dpr = window().device_pixel_ratio().max(0.5);
        let scale = self.zoom * dpr;
        if scale >= 1.0 {
            1
        } else {
            clamp((1.0 / scale).floor(), 1.0, MAX_SAMPLE_STEP as f64) as i32
        }
    }

    /// The part of the world the canvas can currently show, in world pixels,
    /// padded by a cell so nothing pops in at the edge. This is what the
    /// settlement composites and what gets uploaded, so the cost of a frame
    /// follows the size of the window rather than the size of the map.
    pub fn visible_rect(&self, world: &World) -> Rect {
        let (rw, rh) = self.rect();
        if rw <= 0.0 || rh <= 0.0 || self.zoom <= 0.0 {
            return Rect::whole(world);
        }
        let pad = (world.cell_px * 4) as f64;
        let x0 = ((-self.pan_x / self.zoom) - pad).floor().max(0.0) as i32;
        let y0 = ((-self.pan_y / self.zoom) - pad).floor().max(0.0) as i32;
        let x1 = (((rw - self.pan_x) / self.zoom) + pad).ceil().min(world.px_w as f64) as i32;
        let y1 = (((rh - self.pan_y) / self.zoom) + pad).ceil().min(world.px_h as f64) as i32;
        Rect { x0: x0.min(world.px_w), y0: y0.min(world.px_h), x1: x1.max(0), y1: y1.max(0) }
    }

    /// Hands the camera this frame's cloud tile, so the empty space around
    /// the map is the same sky the map hangs in. The tile is uploaded only
    /// when its key moved; the drift is applied at draw time, so a frame in
    /// which only the drift changed uploads nothing.
    pub fn set_space_clouds(
        &mut self,
        layer: &crate::civ::clouds::CloudLayer,
        cfg: &crate::world::WorldConfig,
        cloud_top: i32,
    ) {
        if layer.px.is_empty() {
            self.space_clouds = None;
            return;
        }
        if self.clouds_key != layer.key {
            if self.clouds_tile.width() != layer.w as u32
                || self.clouds_tile.height() != layer.h as u32
            {
                self.clouds_tile.set_width(layer.w as u32);
                self.clouds_tile.set_height(layer.h as u32);
            }
            put_buffer(&self.clouds_tile_ctx, &layer.px, layer.w, layer.h);
            self.clouds_key = layer.key;
        }
        self.space_clouds = Some(SpaceClouds {
            drift: layer.drift,
            sky_px: cfg.sky_px,
            cloud_top,
            top: cfg.sky_top.clone(),
            bottom: cfg.sky_bottom.clone(),
        });
    }

    pub fn clear_space_clouds(&mut self) {
        self.space_clouds = None;
    }

    /// The letterbox as sky: the world's own gradient carried past its edges,
    /// with the cloud tile repeated across all of it in world scale. Drawn
    /// under the world, which paints over its own rectangle.
    fn draw_space_clouds(&self, rw: f64, rh: f64) {
        let sc = match &self.space_clouds {
            Some(sc) => sc,
            None => return,
        };
        let ctx = &self.ctx;
        let g = ctx.create_linear_gradient(
            0.0,
            self.pan_y,
            0.0,
            self.pan_y + sc.sky_px.max(1) as f64 * self.zoom,
        );
        let _ = g.add_color_stop(0.0, &sc.top);
        let _ = g.add_color_stop(1.0, &sc.bottom);
        ctx.set_fill_style_canvas_gradient(&g);
        ctx.fill_rect(0.0, 0.0, rw, rh);
        let pattern = match ctx.create_pattern_with_html_canvas_element(&self.clouds_tile, "repeat")
        {
            Ok(Some(p)) => p,
            _ => return,
        };
        // A pattern repeats in the context's current space, so the context is
        // put into world scale and the visible rectangle is filled in world
        // coordinates; the tile then lands exactly where the sky band's own
        // stamping put it.
        ctx.save();
        ctx.translate(self.pan_x, self.pan_y).ok();
        ctx.scale(self.zoom, self.zoom).ok();
        // The tile's first row lands on the cloud line rather than on the top
        // of the world, which is where the map's own band anchors it too, so
        // one shape carries on across the edge of the map.
        ctx.translate(-sc.drift as f64, sc.cloud_top as f64).ok();
        ctx.set_fill_style_canvas_pattern(&pattern);
        if self.zoom > 0.0 {
            // Local coordinates now count from the cloud line down; nothing
            // above it is filled, so the sky over the weather stays clear.
            let top = ((-self.pan_y) / self.zoom - sc.cloud_top as f64).max(0.0);
            let bottom = (rh - self.pan_y) / self.zoom - sc.cloud_top as f64;
            if bottom > top {
                ctx.fill_rect(
                    (-self.pan_x) / self.zoom + sc.drift as f64,
                    top,
                    rw / self.zoom,
                    bottom - top,
                );
            }
        }
        ctx.restore();
    }

    /// The finished world buffer, scaled onto the canvas with no smoothing.
    pub fn present(&mut self, world: &World, buf: &[u32]) {
        let all = Rect::whole(world);
        self.present_region(world, buf, all);
    }

    /// A plain buffer of `w` by `h` pixels, through the same camera. The sprite
    /// editor's surface is not a world - no cells, no sky, no ground plane - so
    /// it says its own size rather than borrowing a world to carry it.
    pub fn present_flat(&mut self, w: i32, h: i32, buf: &[u32]) {
        self.present_buffer(w, buf, Rect { x0: 0, y0: 0, x1: w, y1: h });
    }

    /// Centers a flat buffer and picks a whole number zoom for it, so a sprite
    /// is drawn at a whole number of screen pixels per art pixel and its edges
    /// stay where they were drawn.
    pub fn fit_flat(&mut self, w: i32, h: i32) {
        let (rw, rh) = self.rect();
        if rw <= 0.0 || rh <= 0.0 || w <= 0 || h <= 0 {
            return;
        }
        self.zoom = self.fit_flat_zoom(w, h);
        self.pan_x = (rw - w as f64 * self.zoom) / 2.0;
        self.pan_y = (rh - h as f64 * self.zoom) / 2.0;
    }

    /// The same, without moving the camera.
    pub fn fit_flat_zoom(&self, w: i32, h: i32) -> f64 {
        let (rw, rh) = self.rect();
        if rw <= 0.0 || rh <= 0.0 || w <= 0 || h <= 0 {
            return self.zoom;
        }
        // A margin, so the sheet is not drawn edge to edge against the stage,
        // and a whole number so a sprite is a whole number of screen pixels an
        // art pixel.
        let fit = (rw / w as f64).min(rh / h as f64) * 0.85;
        clamp(fit.floor().max(1.0), 1.0, 64.0)
    }

    /// Where a pointer is, as a pixel of a flat buffer. Off the buffer reads as
    /// nothing rather than as the nearest edge, so a stroke that leaves the art
    /// stops instead of drawing down the side of it.
    pub fn flat_cell_at(&self, client_x: f64, client_y: f64, w: i32, h: i32) -> Option<(i32, i32)> {
        let r = self.canvas.get_bounding_client_rect();
        if self.zoom <= 0.0 {
            return None;
        }
        let x = ((client_x - r.left() - self.pan_x) / self.zoom).floor() as i32;
        let y = ((client_y - r.top() - self.pan_y) / self.zoom).floor() as i32;
        if x < 0 || y < 0 || x >= w || y >= h {
            return None;
        }
        Some((x, y))
    }

    /// Where a pointer is on the ground plane, in cells, fractional. Not
    /// clamped to the map and not rounded to a cell: a person stands at a
    /// point rather than in the middle of a square, and whether the point is
    /// on the map at all is the caller's question.
    /// Where a press landed in the picture itself, in world pixels: the sky
    /// band included, which the ground coordinates cannot say because there is
    /// no row above the first one.
    pub fn frame_at(&self, client_x: f64, client_y: f64) -> Option<(f64, f64)> {
        if self.zoom <= 0.0 {
            return None;
        }
        let r = self.canvas.get_bounding_client_rect();
        Some((
            (client_x - r.left() - self.pan_x) / self.zoom,
            (client_y - r.top() - self.pan_y) / self.zoom,
        ))
    }

    pub fn ground_at(&self, client_x: f64, client_y: f64, world: &World) -> Option<(f64, f64)> {
        if self.zoom <= 0.0 || world.cell_px <= 0 || world.depth_px <= 0 {
            return None;
        }
        let r = self.canvas.get_bounding_client_rect();
        let px = (client_x - r.left() - self.pan_x) / self.zoom;
        let py = (client_y - r.top() - self.pan_y) / self.zoom;
        Some((px / world.cell_px as f64, (py - world.sky_px as f64) / world.depth_px as f64))
    }

    /// One hairline per art pixel, and a border around the sheet. Drawn only
    /// once the pixels are large enough for a grid to read as a grid rather
    /// than as a screen door.
    pub fn draw_pixel_grid(&self, w: i32, h: i32) {
        let ctx = &self.ctx;
        let (x0, y0) = (self.pan_x, self.pan_y);
        let (x1, y1) = (x0 + w as f64 * self.zoom, y0 + h as f64 * self.zoom);
        if self.zoom >= 6.0 {
            ctx.set_stroke_style_str("rgba(255,255,255,0.10)");
            ctx.set_line_width(1.0);
            ctx.begin_path();
            for x in 1..w {
                let px = (x0 + x as f64 * self.zoom).round() + 0.5;
                ctx.move_to(px, y0);
                ctx.line_to(px, y1);
            }
            for y in 1..h {
                let py = (y0 + y as f64 * self.zoom).round() + 0.5;
                ctx.move_to(x0, py);
                ctx.line_to(x1, py);
            }
            ctx.stroke();
        }
        ctx.set_stroke_style_str("rgba(255,201,120,0.5)");
        ctx.set_line_width(1.0);
        ctx.stroke_rect(x0.round() + 0.5, y0.round() + 0.5, x1 - x0, y1 - y0);
    }

    /// The same, uploading only one rectangle of the buffer, and below one to
    /// one only one pixel of each block the canvas would collapse. A map big
    /// enough to be worth having has a buffer far too large to push to a canvas
    /// sixty times a second, and zoomed out most of what would be pushed is
    /// thrown away on arrival.
    pub fn present_region(&mut self, world: &World, buf: &[u32], rect: Rect) {
        self.present_buffer(world.px_w, buf, rect);
    }

    /// The shared part: the row stride is all a buffer has to say about itself.
    fn present_buffer(&mut self, px_w: i32, buf: &[u32], rect: Rect) {
        self.resize();
        let ctx = &self.ctx;
        ctx.save();
        let _ = ctx.set_transform(self.dpr, 0.0, 0.0, self.dpr, 0.0, 0.0);
        let (rw, rh) = self.rect();
        ctx.clear_rect(0.0, 0.0, rw, rh);
        ctx.set_fill_style_str("#05070a");
        ctx.fill_rect(0.0, 0.0, rw, rh);
        ctx.set_image_smoothing_enabled(false);
        self.draw_space_clouds(rw, rh);
        if rect.is_empty() {
            return;
        }
        // The sampling grid starts at the first multiple of the step inside the
        // region, so a sample always stands for the block of world pixels that
        // begins at it.
        let step = self.sample_step();
        let x0 = round_up(rect.x0, step);
        let y0 = round_up(rect.y0, step);
        if x0 >= rect.x1 || y0 >= rect.y1 {
            return;
        }
        let w = (rect.x1 - x0 + step - 1) / step;
        let h = (rect.y1 - y0 + step - 1) / step;
        if self.off.width() != w as u32 || self.off.height() != h as u32 {
            self.off.set_width(w as u32);
            self.off.set_height(h as u32);
        }
        self.scratch.resize((w * h) as usize, 0);
        for y in 0..h {
            let src = ((y0 + y * step) * px_w) as usize;
            let dst = (y * w) as usize;
            let out = &mut self.scratch[dst..dst + w as usize];
            if step == 1 {
                let src = src + x0 as usize;
                out.copy_from_slice(&buf[src..src + w as usize]);
            } else {
                let row = &buf[src..];
                for (i, px) in out.iter_mut().enumerate() {
                    *px = row[x0 as usize + i * step as usize];
                }
            }
        }
        put_buffer(&self.off_ctx, &self.scratch, w, h);
        let _ = self.ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
            &self.off,
            self.pan_x + x0 as f64 * self.zoom,
            self.pan_y + y0 as f64 * self.zoom,
            (w * step) as f64 * self.zoom,
            (h * step) as f64 * self.zoom,
        );
    }

    pub fn finish(&self) {
        self.ctx.restore();
    }

    /// The ground plane is axis aligned but foreshortened, so cells are drawn
    /// as rectangles cell_px wide by depth_px tall, offset below the sky band.
    pub fn draw_grid(&self, world: &World) {
        let step_x = world.cell_px as f64 * self.zoom;
        let step_y = world.depth_px as f64 * self.zoom;
        if step_x.min(step_y) < 2.0 {
            return;
        }
        let ctx = &self.ctx;
        let top = self.pan_y + world.sky_px as f64 * self.zoom;
        ctx.set_stroke_style_str("rgba(255,255,255,0.10)");
        ctx.set_line_width(1.0);
        ctx.begin_path();
        for x in 0..=world.cols {
            let px = (self.pan_x + x as f64 * step_x).round() + 0.5;
            ctx.move_to(px, top);
            ctx.line_to(px, top + world.ground_px as f64 * self.zoom);
        }
        for y in 0..=world.rows {
            let py = (top + y as f64 * step_y).round() + 0.5;
            ctx.move_to(self.pan_x, py);
            ctx.line_to(self.pan_x + world.px_w as f64 * self.zoom, py);
        }
        ctx.stroke();
        ctx.set_stroke_style_str("rgba(255,180,90,0.55)");
        ctx.begin_path();
        ctx.move_to(self.pan_x, top.round() + 0.5);
        ctx.line_to(self.pan_x + world.px_w as f64 * self.zoom, top.round() + 0.5);
        ctx.stroke();
    }

    pub fn draw_occupancy(&self, world: &World) {
        let step_x = world.cell_px as f64 * self.zoom;
        let step_y = world.depth_px as f64 * self.zoom;
        let top = self.pan_y + world.sky_px as f64 * self.zoom;
        let ctx = &self.ctx;
        for cy in 0..world.rows {
            for cx in 0..world.cols {
                let mask = world.occupancy_at(cx, cy);
                if mask == 0 {
                    continue;
                }
                for l in 0..LAYER_COUNT {
                    if mask & (1 << l) == 0 {
                        continue;
                    }
                    ctx.set_fill_style_str(LAYER_COLORS[l % LAYER_COLORS.len()]);
                    let inset_x = (step_x / (LAYER_COUNT + 1) as f64) * l as f64;
                    let inset_y = (step_y / (LAYER_COUNT + 1) as f64) * l as f64;
                    ctx.fill_rect(
                        self.pan_x + cx as f64 * step_x + inset_x * 0.5,
                        top + cy as f64 * step_y + inset_y * 0.5,
                        (step_x - inset_x).max(1.0),
                        (step_y - inset_y).max(1.0),
                    );
                }
            }
        }
    }

    /// The night tint and the labels are drawn on the canvas rather than into
    /// the pixel buffer: darkening 300k pixels per frame by hand is not worth
    /// it when one translucent rectangle does the same job.
    pub fn draw_civ_overlay(&self, sim: &Settlement, state: &State) {
        let world = sim.world();
        let w = world.px_w as f64 * self.zoom;
        let h = world.px_h as f64 * self.zoom;
        let ctx = &self.ctx;
        if state.civ.view.day_night {
            let light = sim.daylight(state);
            if light < 0.95 {
                let dark = (1.0 - light) * 0.55;
                let tint = unpack_rgba(hex_to_packed(&state.civ.world.sky_top));
                ctx.set_fill_style_str(&format!(
                    "rgba({}, {}, {}, {:.3})",
                    tint.r, tint.g, tint.b, dark
                ));
                if self.space_clouds.is_some() {
                    // The letterbox is sky too, so the night falls on all of
                    // it rather than stopping at the map's edge.
                    let (rw, rh) = self.rect();
                    ctx.fill_rect(0.0, 0.0, rw, rh);
                } else {
                    ctx.fill_rect(self.pan_x, self.pan_y, w, h);
                }
                self.draw_lamps(sim, 1.0 - light);
            }
        }
        if !state.civ.view.labels {
            return;
        }
        let labels = building_labels(sim, &state.civ.view);
        if labels.is_empty() {
            return;
        }
        let size = (7.0 * self.zoom.min(3.0)).round().max(9.0);
        ctx.set_font(&format!("{size}px ui-monospace, monospace"));
        ctx.set_text_align("center");
        ctx.set_fill_style_str("rgba(230, 236, 245, 0.85)");
        ctx.set_stroke_style_str("rgba(6, 10, 16, 0.9)");
        ctx.set_line_width(3.0);
        for (x, y, text) in labels {
            let sx = self.pan_x + x * self.zoom;
            let sy = self.pan_y + y * self.zoom + 10.0;
            let _ = ctx.stroke_text(&text, sx, sy);
            let _ = ctx.fill_text(&text, sx, sy);
        }
    }

    /// Pools of light under the lamps, drawn over the night tint rather than
    /// cut out of it: a lamp gives light off, so adding one is both the simpler
    /// operation and the truer one. Nothing is drawn by day, and each pool
    /// strengthens with the dark that it is pushing back.
    ///
    /// The pools are screened together rather than summed. A street of lamps
    /// overlaps a lot, and pure addition stacks the overlaps past the color of
    /// any one flame and out to white; screening converges on that color
    /// instead, so a well lit square is warm rather than blown out.
    fn draw_lamps(&self, sim: &Settlement, dark: f64) {
        let ctx = &self.ctx;
        let mut lit = false;
        for (x, y, radius) in lamp_lights(sim) {
            let sx = self.pan_x + x * self.zoom;
            let sy = self.pan_y + y * self.zoom;
            let sr = radius * self.zoom;
            if sr < 1.0 {
                continue;
            }
            let (rw, rh) = self.rect();
            if sx + sr < 0.0 || sy + sr < 0.0 || sx - sr > rw || sy - sr > rh {
                continue;
            }
            let grad = match ctx.create_radial_gradient(sx, sy, 0.0, sx, sy, sr) {
                Ok(g) => g,
                Err(_) => continue,
            };
            let core = (0.55 * dark).clamp(0.0, 0.6);
            let _ = grad.add_color_stop(0.0, &format!("rgba(255, 214, 138, {core:.3})"));
            let _ = grad.add_color_stop(0.45, &format!("rgba(255, 190, 110, {:.3})", core * 0.35));
            let _ = grad.add_color_stop(1.0, "rgba(255, 180, 100, 0)");
            if !lit {
                ctx.set_global_composite_operation("screen").ok();
                lit = true;
            }
            ctx.set_fill_style_canvas_gradient(&grad);
            ctx.fill_rect(sx - sr, sy - sr, sr * 2.0, sr * 2.0);
        }
        if lit {
            ctx.set_global_composite_operation("source-over").ok();
        }
    }

    /// What the hand tool shows: a slow pulse round everything that could be
    /// cut, and a bar over whatever is being cut right now.
    ///
    /// Drawn on the canvas over the finished frame rather than into it. Both
    /// halves change every frame while nothing about the world does, and a
    /// pulse composited into the pixel buffer would mean repainting the band
    /// around every plant on screen sixty times a second.
    pub fn draw_harvest_overlay(
        &self,
        sim: &Settlement,
        state: &State,
        hover: Option<i32>,
        ts: f64,
    ) {
        let marks = harvest_marks(sim, state, HARVEST_MARK_LIMIT, hover);
        if marks.is_empty() {
            return;
        }
        let ctx = &self.ctx;
        // Slow enough to read as breathing rather than blinking.
        let pulse = (ts / 1000.0 * std::f64::consts::TAU / PULSE_SECONDS).sin() * 0.5 + 0.5;
        let wash = 0.10 + pulse * 0.13;
        let lit = 0.45 + pulse * 0.35;
        ctx.set_line_width((self.zoom * 0.5).clamp(1.0, 2.5));
        for hot in [false, true] {
            ctx.set_stroke_style_str(&format!(
                "rgba(168, 232, 150, {:.3})",
                if hot { lit } else { wash }
            ));
            for mark in marks.iter().filter(|m| m.hot == hot) {
                let rx = mark.half_w * self.zoom + 2.0;
                let ry = mark.height * 0.5 * self.zoom + 2.0;
                if rx < 1.5 || ry < 1.5 {
                    continue;
                }
                let sx = self.pan_x + mark.x * self.zoom;
                let sy = self.pan_y + (mark.y - mark.height * 0.5) * self.zoom;
                ctx.begin_path();
                let _ = ctx.ellipse(sx, sy, rx, ry, 0.0, 0.0, std::f64::consts::TAU);
                ctx.stroke();
            }
        }
        for mark in marks.iter().filter(|m| m.cut.is_some()) {
            let (done, alpha) = match mark.cut {
                Some(cut) => cut,
                None => continue,
            };
            // Held clear of the top of the plant, and never smaller than a bar
            // somebody could read: this is the one thing on screen that says
            // how much longer to hold on for.
            let w = (mark.half_w * 2.0 * self.zoom).clamp(18.0, 64.0);
            let h = 4.0;
            let sx = self.pan_x + mark.x * self.zoom - w / 2.0;
            let sy = self.pan_y + (mark.y - mark.height) * self.zoom - h * 2.5;
            // The track is outlined as well as filled: an unlit bar over dark
            // foliage is the same color as the foliage, and the length left to
            // go is half of what the bar is saying.
            ctx.set_fill_style_str(&format!("rgba(8, 12, 18, {:.3})", 0.7 * alpha));
            ctx.fill_rect(sx - 1.0, sy - 1.0, w + 2.0, h + 2.0);
            ctx.set_line_width(1.0);
            ctx.set_stroke_style_str(&format!("rgba(214, 232, 210, {:.3})", 0.5 * alpha));
            ctx.stroke_rect(sx - 0.5, sy - 0.5, w + 1.0, h + 1.0);
            ctx.set_fill_style_str(&format!("rgba(196, 244, 168, {alpha:.3})"));
            ctx.fill_rect(sx, sy, w * done, h);
        }
    }

    /// Town names over their centers, when labels are on and there is more than
    /// one town to tell apart.
    pub fn draw_colony_labels(&self, sim: &Settlement, state: &State) {
        if !state.civ.view.label_on(None) || sim.colonies.len() < 2 {
            return;
        }
        let ctx = &self.ctx;
        let size = (9.0 * self.zoom.clamp(0.6, 2.0)).round().max(11.0);
        ctx.set_font(&format!("{size}px ui-monospace, monospace"));
        ctx.set_text_align("center");
        ctx.set_stroke_style_str("rgba(6, 10, 16, 0.9)");
        ctx.set_line_width(3.5);
        for (x, y, text, banner) in colony_labels(sim) {
            let c = unpack_rgba(banner);
            let sx = self.pan_x + x * self.zoom;
            let sy = self.pan_y + y * self.zoom - 6.0;
            let _ = ctx.stroke_text(&text, sx, sy);
            ctx.set_fill_style_str(&format!("rgb({}, {}, {})", c.r, c.g, c.b));
            let _ = ctx.fill_text(&text, sx, sy);
        }
    }
}

/// Draws a single plant sprite centered in a canvas: used by the species
/// preview and by any place that needs to show one specimen.
pub fn draw_plant_preview(canvas: &HtmlCanvasElement, plant: &Plant) {
    let ctx = context_of(canvas);
    let r = canvas.get_bounding_client_rect();
    let (rw, rh) = (r.width(), r.height());
    let dpr = window().device_pixel_ratio();
    let w = ((rw * dpr).round() as u32).max(1);
    let h = ((rh * dpr).round() as u32).max(1);
    if canvas.width() != w || canvas.height() != h {
        canvas.set_width(w);
        canvas.set_height(h);
    }
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.clear_rect(0.0, 0.0, rw, rh);
    ctx.set_fill_style_str("#0b0f14");
    ctx.fill_rect(0.0, 0.0, rw, rh);
    if plant.bounds.is_empty() || rw == 0.0 {
        return;
    }

    let off = new_canvas();
    off.set_width(plant.w as u32);
    off.set_height(plant.h as u32);
    let octx = context_of(&off);
    put_buffer(&octx, &plant.sprite, plant.w, plant.h);

    // Framed on the whole sprite rather than the current silhouette, so the
    // view does not rescale on every growth step.
    let z = clamp(
        (rw / plant.w as f64).min(rh / plant.h as f64).floor(),
        1.0,
        10.0,
    );
    let dx = ((rw - plant.w as f64 * z) / 2.0).round();
    let dy = ((rh - plant.h as f64 * z) / 2.0).round();
    ctx.set_image_smoothing_enabled(false);
    let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        &off,
        0.0,
        0.0,
        plant.w as f64,
        plant.h as f64,
        dx,
        dy,
        plant.w as f64 * z,
        plant.h as f64 * z,
    );
}
