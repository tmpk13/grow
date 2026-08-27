//! Sprite sheets authored in the tool: frames, layers and the pixels in them.
//!
//! A sheet is one frame size and a stack of layers, each holding one cel per
//! frame. Drawing happens on a single cel; what the map and the previews read
//! is the flattened frame, which is every visible layer laid over the one below
//! it. Layers only ever hold opaque pixels or nothing at all, so flattening is
//! a copy of whatever the topmost visible layer has there.
//!
//! Sheets live in the project file, which lives in local storage, so cels are
//! stored run length encoded: sprite art is mostly empty and mostly flat, and
//! the plain pixel-per-hex-quad form the sampling boxes use would spend eight
//! characters on every transparent pixel.

use serde::{Deserialize, Serialize};

use crate::util::{hsl_to_packed, EMPTY_COLOR};

/// Caps on one sheet. A frame past the size cap is not something the map can
/// draw, and the rest is what keeps a project small enough to save.
pub const MAX_SHEET_PX: i32 = 64;
pub const MAX_SHEET_FRAMES: i32 = 24;
pub const MAX_LAYERS: usize = 8;

/// Longest run the encoding can carry, because a run length is two hex digits.
const MAX_RUN: usize = 255;

/// Cels travel as runs of `<count><rgba>`, two hex digits then eight.
pub mod px_rle {
    use super::MAX_RUN;
    use crate::util::{packed_to_rgba_hex, rgba_hex_to_packed};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(px: &[u32], s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::new();
        let mut at = 0;
        while at < px.len() {
            let v = px[at];
            let mut run = 1;
            while at + run < px.len() && px[at + run] == v && run < MAX_RUN {
                run += 1;
            }
            out.push_str(&format!("{run:02x}"));
            out.push_str(&packed_to_rgba_hex(v));
            at += run;
        }
        s.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u32>, D::Error> {
        let raw = String::deserialize(d)?;
        let mut out = Vec::new();
        let mut at = 0;
        while at + 10 <= raw.len() {
            let run = usize::from_str_radix(&raw[at..at + 2], 16).unwrap_or(0);
            let v = rgba_hex_to_packed(&raw[at + 2..at + 10]);
            for _ in 0..run {
                out.push(v);
            }
            at += 10;
        }
        Ok(out)
    }
}

/// One layer's pixels for one frame.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Cel {
    #[serde(with = "px_rle")]
    pub px: Vec<u32>,
}

impl Cel {
    pub fn blank(n: usize) -> Cel {
        Cel { px: vec![EMPTY_COLOR; n] }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// One cel per frame of the sheet, each `w * h` pixels.
    pub cels: Vec<Cel>,
}

impl Default for Layer {
    fn default() -> Self {
        Layer { name: "Layer".to_string(), visible: true, cels: Vec::new() }
    }
}

/// One animation: a frame size, a stack of layers and how fast it plays.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sheet {
    pub id: String,
    pub name: String,
    pub w: i32,
    pub h: i32,
    pub frames: i32,
    /// Frames per second the editor's preview runs at, and what a clip built
    /// from this sheet starts on.
    pub fps: f64,
    /// Bottom of the stack first, so the last layer is the one on top.
    pub layers: Vec<Layer>,
}

impl Default for Sheet {
    fn default() -> Self {
        Sheet {
            id: String::new(),
            name: "Sheet".to_string(),
            w: 16,
            h: 16,
            frames: 1,
            fps: 6.0,
            layers: Vec::new(),
        }
    }
}

impl Sheet {
    pub fn new(id: &str, name: &str, w: i32, h: i32) -> Sheet {
        let mut sheet = Sheet {
            id: id.to_string(),
            name: name.to_string(),
            w: w.clamp(1, MAX_SHEET_PX),
            h: h.clamp(1, MAX_SHEET_PX),
            frames: 1,
            fps: 6.0,
            layers: vec![Layer { name: "Base".to_string(), visible: true, cels: Vec::new() }],
        };
        sheet.fit();
        sheet
    }

    pub fn cel_len(&self) -> usize {
        (self.w.max(1) * self.h.max(1)) as usize
    }

    /// Brings the sheet back to its own invariants: sizes in range, at least
    /// one layer, and every layer holding exactly one cel of the right length
    /// per frame. Everything that edits a sheet ends here, and so does loading
    /// a project somebody has hand edited.
    pub fn fit(&mut self) {
        self.w = self.w.clamp(1, MAX_SHEET_PX);
        self.h = self.h.clamp(1, MAX_SHEET_PX);
        self.frames = self.frames.clamp(1, MAX_SHEET_FRAMES);
        self.fps = self.fps.clamp(0.0, 24.0);
        if self.layers.is_empty() {
            self.layers.push(Layer::default());
        }
        self.layers.truncate(MAX_LAYERS);
        let len = self.cel_len();
        let frames = self.frames as usize;
        for layer in &mut self.layers {
            layer.cels.resize_with(frames, || Cel::blank(len));
            for cel in &mut layer.cels {
                cel.px.resize(len, EMPTY_COLOR);
            }
        }
    }

    pub fn frame_count(&self) -> i32 {
        self.frames.clamp(1, MAX_SHEET_FRAMES)
    }

    pub fn get(&self, layer: usize, frame: i32, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return EMPTY_COLOR;
        }
        self.layers
            .get(layer)
            .and_then(|l| l.cels.get(frame.max(0) as usize))
            .and_then(|c| c.px.get((y * self.w + x) as usize))
            .copied()
            .unwrap_or(EMPTY_COLOR)
    }

    pub fn set(&mut self, layer: usize, frame: i32, x: i32, y: i32, v: u32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let w = self.w;
        if let Some(cel) = self
            .layers
            .get_mut(layer)
            .and_then(|l| l.cels.get_mut(frame.max(0) as usize))
        {
            if let Some(slot) = cel.px.get_mut((y * w + x) as usize) {
                *slot = v;
            }
        }
    }

    /// One frame with every visible layer laid over the one below it.
    pub fn flatten(&self, frame: i32) -> Vec<u32> {
        let mut out = vec![EMPTY_COLOR; self.cel_len()];
        let frame = frame.clamp(0, self.frame_count() - 1) as usize;
        for layer in self.layers.iter().filter(|l| l.visible) {
            let cel = match layer.cels.get(frame) {
                Some(c) => c,
                None => continue,
            };
            for (i, v) in cel.px.iter().enumerate() {
                if *v != EMPTY_COLOR && i < out.len() {
                    out[i] = *v;
                }
            }
        }
        out
    }

    /// Every frame side by side, which is the shape a clip is read from.
    pub fn strip(&self) -> (i32, i32, Vec<u32>) {
        let frames = self.frame_count();
        let w = self.w * frames;
        let mut px = vec![EMPTY_COLOR; (w * self.h) as usize];
        for f in 0..frames {
            let flat = self.flatten(f);
            for y in 0..self.h {
                for x in 0..self.w {
                    px[(y * w + f * self.w + x) as usize] = flat[(y * self.w + x) as usize];
                }
            }
        }
        (w, self.h, px)
    }

    /// True once anything has been drawn anywhere. An empty sheet is not worth
    /// pointing a settler at.
    pub fn any(&self) -> bool {
        self.layers
            .iter()
            .any(|l| l.cels.iter().any(|c| c.px.iter().any(|v| *v != EMPTY_COLOR)))
    }

    // ---- frames ----------------------------------------------------------

    /// Copies the frame at `at` into the slot after it, so a pose can be
    /// nudged rather than redrawn. An empty frame is added when `copy` is off.
    pub fn add_frame(&mut self, at: i32, copy: bool) -> i32 {
        if self.frame_count() >= MAX_SHEET_FRAMES {
            return at;
        }
        let at = at.clamp(0, self.frame_count() - 1) as usize;
        let len = self.cel_len();
        for layer in &mut self.layers {
            let cel = if copy {
                layer.cels.get(at).cloned().unwrap_or_else(|| Cel::blank(len))
            } else {
                Cel::blank(len)
            };
            layer.cels.insert((at + 1).min(layer.cels.len()), cel);
        }
        self.frames += 1;
        self.fit();
        (at + 1) as i32
    }

    pub fn remove_frame(&mut self, at: i32) -> i32 {
        if self.frame_count() <= 1 {
            return 0;
        }
        let at = at.clamp(0, self.frame_count() - 1) as usize;
        for layer in &mut self.layers {
            if at < layer.cels.len() {
                layer.cels.remove(at);
            }
        }
        self.frames -= 1;
        self.fit();
        (at.max(1) - 1) as i32
    }

    /// Swaps a frame with its neighbour, taking every layer with it.
    pub fn move_frame(&mut self, at: i32, delta: i32) -> i32 {
        let n = self.frame_count();
        let at = at.clamp(0, n - 1);
        let to = at + delta;
        if to < 0 || to >= n {
            return at;
        }
        for layer in &mut self.layers {
            layer.cels.swap(at as usize, to as usize);
        }
        to
    }

    // ---- layers ----------------------------------------------------------

    /// Adds an empty layer above `at` and returns where it landed.
    pub fn add_layer(&mut self, at: usize, name: &str) -> usize {
        if self.layers.len() >= MAX_LAYERS {
            return at;
        }
        let at = (at + 1).min(self.layers.len());
        self.layers.insert(
            at,
            Layer { name: name.to_string(), visible: true, cels: Vec::new() },
        );
        self.fit();
        at
    }

    pub fn remove_layer(&mut self, at: usize) -> usize {
        if self.layers.len() <= 1 || at >= self.layers.len() {
            return at.min(self.layers.len().saturating_sub(1));
        }
        self.layers.remove(at);
        self.fit();
        at.saturating_sub(1)
    }

    pub fn move_layer(&mut self, at: usize, delta: i32) -> usize {
        let to = at as i32 + delta;
        if at >= self.layers.len() || to < 0 || to as usize >= self.layers.len() {
            return at;
        }
        self.layers.swap(at, to as usize);
        to as usize
    }

    /// Folds a layer into the one below it, in every frame, and drops it. The
    /// upper layer wins wherever both have a pixel, which is how they are
    /// drawn.
    pub fn merge_down(&mut self, at: usize) -> usize {
        if at == 0 || at >= self.layers.len() {
            return at;
        }
        let upper = self.layers[at].clone();
        {
            let lower = &mut self.layers[at - 1];
            for (f, cel) in upper.cels.iter().enumerate() {
                let target = match lower.cels.get_mut(f) {
                    Some(t) => t,
                    None => continue,
                };
                for (i, v) in cel.px.iter().enumerate() {
                    if *v != EMPTY_COLOR && i < target.px.len() {
                        target.px[i] = *v;
                    }
                }
            }
        }
        self.layers.remove(at);
        self.fit();
        at - 1
    }

    // ---- whole-sheet edits ----------------------------------------------

    /// Crops or pads every cel. Pixel art does not survive being resampled, so
    /// the art keeps its position and the new room is empty.
    pub fn resize(&mut self, w: i32, h: i32) {
        let (nw, nh) = (w.clamp(1, MAX_SHEET_PX), h.clamp(1, MAX_SHEET_PX));
        if nw == self.w && nh == self.h {
            return;
        }
        let (ow, oh) = (self.w, self.h);
        for layer in &mut self.layers {
            for cel in &mut layer.cels {
                let mut next = vec![EMPTY_COLOR; (nw * nh) as usize];
                for y in 0..nh.min(oh) {
                    for x in 0..nw.min(ow) {
                        next[(y * nw + x) as usize] = cel.px[(y * ow + x) as usize];
                    }
                }
                cel.px = next;
            }
        }
        self.w = nw;
        self.h = nh;
        self.fit();
    }

    /// Mirrors one cel, or every cel in the sheet, left to right.
    pub fn flip_cel(&mut self, layer: usize, frame: i32) {
        let (w, h) = (self.w, self.h);
        if let Some(cel) = self
            .layers
            .get_mut(layer)
            .and_then(|l| l.cels.get_mut(frame.max(0) as usize))
        {
            flip_px(&mut cel.px, w, h);
        }
    }

    pub fn flip_all(&mut self) {
        let (w, h) = (self.w, self.h);
        for layer in &mut self.layers {
            for cel in &mut layer.cels {
                flip_px(&mut cel.px, w, h);
            }
        }
    }

    /// Shifts one cel, or every cel in the sheet, by whole pixels. What moves
    /// off an edge is gone: a sheet is the frame, not a window onto something
    /// larger, and keeping what has left it would mean carrying a buffer nobody
    /// can see.
    pub fn shift_cel(&mut self, layer: usize, frame: i32, dx: i32, dy: i32) {
        let (w, h) = (self.w, self.h);
        if let Some(cel) = self
            .layers
            .get_mut(layer)
            .and_then(|l| l.cels.get_mut(frame.max(0) as usize))
        {
            shift_px(&mut cel.px, w, h, dx, dy);
        }
    }

    pub fn shift_all(&mut self, dx: i32, dy: i32) {
        let (w, h) = (self.w, self.h);
        for layer in &mut self.layers {
            for cel in &mut layer.cels {
                shift_px(&mut cel.px, w, h, dx, dy);
            }
        }
    }

    pub fn clear_cel(&mut self, layer: usize, frame: i32) {
        if let Some(cel) = self
            .layers
            .get_mut(layer)
            .and_then(|l| l.cels.get_mut(frame.max(0) as usize))
        {
            cel.px.fill(EMPTY_COLOR);
        }
    }

    /// Lays an image into one cel, scaled down to fit the frame and centered in
    /// it. Nearest sampling and never scaled up, so pixel art arrives as its own
    /// pixels wherever it already fits and loses whole ones where it does not.
    /// The cel is cleared first: what is dropped is what the layer then shows,
    /// which is the only reading of a drop that does not depend on what was
    /// underneath.
    pub fn place(&mut self, layer: usize, frame: i32, src_w: i32, src_h: i32, px: &[u32]) {
        if src_w <= 0 || src_h <= 0 {
            return;
        }
        self.clear_cel(layer, frame);
        let ratio = (src_w as f64 / self.w as f64)
            .max(src_h as f64 / self.h as f64)
            .max(1.0);
        let w = ((src_w as f64 / ratio).round() as i32).clamp(1, self.w);
        let h = ((src_h as f64 / ratio).round() as i32).clamp(1, self.h);
        let (ox, oy) = ((self.w - w) / 2, (self.h - h) / 2);
        for y in 0..h {
            let sy = ((y as i64 * src_h as i64) / h as i64).min(src_h as i64 - 1) as i32;
            for x in 0..w {
                let sx = ((x as i64 * src_w as i64) / w as i64).min(src_w as i64 - 1) as i32;
                let v = match px.get((sy * src_w + sx) as usize) {
                    Some(v) => *v,
                    None => continue,
                };
                if v != EMPTY_COLOR {
                    self.set(layer, frame, ox + x, oy + y, v);
                }
            }
        }
    }

    /// What the sheet costs in a saved project, counting the ten characters a
    /// run of one pixel takes.
    pub fn bytes(&self) -> usize {
        self.layers
            .iter()
            .map(|l| l.cels.iter().map(|c| runs(&c.px) * 10).sum::<usize>())
            .sum()
    }
}

fn shift_px(px: &mut [u32], w: i32, h: i32, dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    let mut next = vec![EMPTY_COLOR; px.len()];
    for y in 0..h {
        let sy = y - dy;
        if sy < 0 || sy >= h {
            continue;
        }
        for x in 0..w {
            let sx = x - dx;
            if sx < 0 || sx >= w {
                continue;
            }
            next[(y * w + x) as usize] = px[(sy * w + sx) as usize];
        }
    }
    px.copy_from_slice(&next);
}

fn flip_px(px: &mut [u32], w: i32, h: i32) {
    for y in 0..h {
        let row = (y * w) as usize;
        for x in 0..w / 2 {
            px.swap(row + x as usize, row + (w - 1 - x) as usize);
        }
    }
}

fn runs(px: &[u32]) -> usize {
    let mut n = 0;
    let mut at = 0;
    while at < px.len() {
        let v = px[at];
        let mut run = 1;
        while at + run < px.len() && px[at + run] == v && run < MAX_RUN {
            run += 1;
        }
        n += 1;
        at += run;
    }
    n
}

/// Every sheet in a project.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ArtLibrary {
    pub sheets: Vec<Sheet>,
}

impl Default for ArtLibrary {
    fn default() -> Self {
        ArtLibrary { sheets: vec![starter_sheet()] }
    }
}

impl ArtLibrary {
    pub fn find(&self, id: &str) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Sheet> {
        self.sheets.iter_mut().find(|s| s.id == id)
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.id == id)
    }

    /// Everything a project loaded from a file might be missing, since the
    /// sheets in it were written by an older build or by hand.
    pub fn fit(&mut self) {
        for sheet in &mut self.sheets {
            sheet.fit();
        }
        if self.sheets.is_empty() {
            self.sheets.push(starter_sheet());
        }
    }

    pub fn bytes(&self) -> usize {
        self.sheets.iter().map(|s| s.bytes()).sum()
    }

    /// The sheets as select options, for the panels that point at one.
    pub fn options(&self) -> Vec<(String, String)> {
        self.sheets
            .iter()
            .map(|s| {
                (
                    s.id.clone(),
                    format!("{} ({}x{}, {} frames)", s.name, s.w, s.h, s.frame_count()),
                )
            })
            .collect()
    }
}

/// The sheet a new project opens on: a settler sized figure, drawn once so the
/// editor has something in it to take apart.
fn starter_sheet() -> Sheet {
    let mut sheet = Sheet::new("art-1", "Settler", 12, 16);
    let skin = hsl_to_packed(28.0, 0.42, 0.66);
    let shirt = hsl_to_packed(206.0, 0.34, 0.44);
    let legs = hsl_to_packed(28.0, 0.24, 0.3);
    for y in 0..16 {
        for x in 0..12 {
            let c = match y {
                0..=1 => EMPTY_COLOR,
                2..=5 if (4..8).contains(&x) => skin,
                6..=10 if (3..9).contains(&x) => shirt,
                11..=15 if (4..8).contains(&x) => legs,
                _ => EMPTY_COLOR,
            };
            if c != EMPTY_COLOR {
                sheet.set(0, 0, x, y, c);
            }
        }
    }
    sheet
}
