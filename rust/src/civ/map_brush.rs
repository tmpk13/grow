//! What a stroke on the map editor means.
//!
//! A brush is one answer about one cell - water here, a rock face there, a
//! wood, a meadow, sky - and nothing else. There is no drawing behind it: the
//! map editor paints the settlement's own map, and these are the colors it
//! paints with and reads back.
//!
//! Two of them are ground, three are what may grow, and one is not land at
//! all. They are one list rather than three menus for the same reason the
//! picture tool's are: the questions are about the same cell, and somebody
//! painting a map is answering whichever one the cell needs.
//!
//! A whole map can be read in from a set of layers, one picture per brush:
//! where a layer has something drawn, the cell is what the layer is. That is
//! how a drawing program hands a map over - a layer of water, a layer of
//! sand, a layer of trees - and it needs no exact colors, which a picture
//! that carried every kind at once did. A layer that covers everything is a
//! layer that says its thing about every cell, which is what a base layer
//! under the rest is.
//!
//! A set of layers says two things at once, and both are kept. Where a layer
//! has something drawn decides what kind of ground the cell is; what was
//! drawn there decides what color it is, the layers flattened being the
//! picture the map is drawn as. The two come apart again cell by cell: paint
//! ground over a cell by hand and the picture comes off that cell, so the
//! drawing and what is drawn by hand are one map rather than two.

use serde::{Deserialize, Serialize};

use crate::civ::sprites::ALPHA_CUT;
use crate::civ::terrain::Cell;
use crate::util::unpack_rgba;
use crate::world::Zone;

/// The map as a picture: the colors under everything on a map that was read
/// out of one. It is stretched over the ground when the ground is drawn, and
/// the generated ground is not drawn where it has something, so a cell that
/// is dirt acts as dirt and looks like whatever was drawn there. Where it is
/// clear the generated ground shows through, which is what land the map grew
/// after the picture was read looks like.
///
/// It is taken off cell by cell as well. A picture read in says two things
/// about every cell it covers - what color it is and, through the layer it
/// was drawn on, what kind of ground it is - and painting over one of those
/// cells by hand answers the second question again. The picture no longer
/// describes that cell, so it comes off it and the ground drawn there shows
/// instead: a lake painted into a drawing is a lake, not a lake-colored patch
/// of whatever the drawing had.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapArt {
    pub w: i32,
    pub h: i32,
    #[serde(with = "crate::art::px_rle")]
    pub px: Vec<u32>,
    /// The cells it has been taken off, one byte a cell, at the size of the
    /// map it is laid on. Empty while it is still on all of them, which is
    /// every map nobody has painted over.
    #[serde(default, with = "off_rle")]
    pub off: Vec<u8>,
}

/// The taken-off cells, written down as runs the way every other byte a cell
/// grid in a settlement is.
mod off_rle {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(off: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::civ::save::bytes_rle(off))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(crate::civ::save::bytes_from_rle(&raw))
    }
}

impl MapArt {
    /// A picture on every cell of whatever map it is laid on.
    pub fn new(w: i32, h: i32, px: Vec<u32>) -> MapArt {
        MapArt { w, h, px, off: Vec::new() }
    }

    /// Whether the picture is drawn on cell `i`, which it is on every cell
    /// until somebody paints ground over one.
    pub fn shown(&self, i: usize) -> bool {
        self.off.get(i) != Some(&1)
    }

    /// Takes it off one cell of a map of `cells` cells, or puts it back. The
    /// grid is only made when the first cell comes off: a picture nobody has
    /// drawn over carries nothing.
    pub fn show(&mut self, i: usize, cells: usize, on: bool) {
        if on && self.off.is_empty() {
            return;
        }
        if self.off.len() != cells {
            self.off = vec![0; cells];
        }
        if let Some(slot) = self.off.get_mut(i) {
            *slot = u8::from(!on);
        }
    }

    /// How many cells it has been taken off.
    pub fn taken_off(&self) -> usize {
        self.off.iter().filter(|&&v| v == 1).count()
    }

    /// The pixel over point `x`, `y` of a ground `of_w` by `of_h` pixels: the
    /// picture stretched corner to corner over it, nearest pixel.
    pub fn at(&self, x: i32, y: i32, of_w: i32, of_h: i32) -> u32 {
        if self.w <= 0 || self.h <= 0 {
            return 0;
        }
        let sx = (x as i64 * self.w as i64 / of_w.max(1) as i64).clamp(0, self.w as i64 - 1);
        let sy = (y as i64 * self.h as i64 / of_h.max(1) as i64).clamp(0, self.h as i64 - 1);
        self.px.get((sy * self.w as i64 + sx) as usize).copied().unwrap_or(0)
    }

    /// The pixel over one cell of a `cols` by `rows` map: the middle of the
    /// cell, which is the point the layers were read at, so what the stage
    /// shows for a cell is the color the map was told that cell is.
    pub fn cell(&self, col: i32, row: i32, cols: i32, rows: i32) -> u32 {
        if self.w <= 0 || self.h <= 0 {
            return 0;
        }
        let x = (((col as f64 + 0.5) / cols.max(1) as f64) * self.w as f64).floor() as i32;
        let y = (((row as f64 + 0.5) / rows.max(1) as f64) * self.h as f64).floor() as i32;
        let x = x.clamp(0, self.w - 1);
        let y = y.clamp(0, self.h - 1);
        self.px.get((y * self.w + x) as usize).copied().unwrap_or(0)
    }

    /// Whether anything in it shows.
    pub fn shows(&self) -> bool {
        self.px.iter().any(|&v| unpack_rgba(v).a >= ALPHA_CUT)
    }

    /// The same picture over a map grown from `old_cols` by `old_rows` to
    /// `cols` by `rows`: what was drawn stays at the top left over the land it
    /// was drawn for, and the new land is clear, so the generated ground
    /// shows there.
    pub fn grown(&self, old_cols: i32, old_rows: i32, cols: i32, rows: i32) -> MapArt {
        let scale = |px: i32, old: i32, new: i32| {
            ((px as f64 * new as f64 / old.max(1) as f64).round() as i32).max(px)
        };
        let w = scale(self.w, old_cols, cols);
        let h = scale(self.h, old_rows, rows);
        let mut px = vec![0u32; (w.max(0) * h.max(0)) as usize];
        for y in 0..self.h.min(h) {
            let n = self.w.min(w) as usize;
            let from = (y * self.w) as usize;
            let to = (y * w) as usize;
            if let (Some(src), Some(dst)) = (self.px.get(from..from + n), px.get_mut(to..to + n)) {
                dst.copy_from_slice(src);
            }
        }
        // The cells it was taken off keep their numbers, the same as
        // everything else standing on the land that was already there.
        let mut off = Vec::new();
        if !self.off.is_empty() {
            off = vec![0u8; (cols.max(0) * rows.max(0)) as usize];
            for r in 0..old_rows.min(rows) {
                for c in 0..old_cols.min(cols) {
                    let was = self.off.get((r * old_cols + c) as usize).copied().unwrap_or(0);
                    if let Some(slot) = off.get_mut((r * cols + c) as usize) {
                        *slot = was;
                    }
                }
            }
        }
        MapArt { w, h, px, off }
    }
}

/// What one press paints.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Brush {
    /// Nothing said. What the eraser paints, and what a color off the wheel is
    /// read back as: it takes a zone off a cell and leaves the ground alone.
    Clear,
    Water,
    /// Ground people walk and build on.
    Rock,
    /// A face of it, which nobody crosses.
    Cliff,
    Grass,
    Sand,
    /// Trees and shrubs only: a wood.
    Wood,
    /// Everything but trees: a meadow.
    Low,
    /// Nothing seeds here at all: a clearing, a yard, a road.
    Bare,
    /// Not land. Sky is a mark on the editor and never on the map: it says
    /// where to read the sky colors out of the picture underneath.
    Sky,
}

/// The brushes, in the order they are shown.
pub const BRUSHES: [Brush; 10] = [
    Brush::Clear,
    Brush::Water,
    Brush::Cliff,
    Brush::Rock,
    Brush::Grass,
    Brush::Sand,
    Brush::Wood,
    Brush::Low,
    Brush::Bare,
    Brush::Sky,
];

impl Brush {
    /// Its place in the list, which is how a picture read in as a map is
    /// carried from the press that read it to the frame that lays it down.
    pub fn id(self) -> u8 {
        BRUSHES.iter().position(|b| *b == self).unwrap_or(0) as u8
    }

    pub fn from_u8(v: u8) -> Brush {
        BRUSHES.get(v as usize).copied().unwrap_or(Brush::Clear)
    }

    pub fn label(self) -> &'static str {
        match self {
            Brush::Clear => "Leave alone",
            Brush::Water => "Water",
            Brush::Rock => "Rock",
            Brush::Cliff => "Rock face",
            Brush::Grass => "Grass",
            Brush::Sand => "Sand",
            Brush::Wood => "Trees only",
            Brush::Low => "Low growth only",
            Brush::Bare => "Nothing grows",
            Brush::Sky => "Sky",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Brush::Clear => "takes the zone off a cell and leaves the ground as it is",
            Brush::Water => "nobody stands on it; boats and swimmers cross it",
            Brush::Rock => "walked on and built on, and nothing much grows",
            Brush::Cliff => "nobody walks in it and nothing takes root",
            Brush::Grass => "ordinary ground",
            Brush::Sand => "shore",
            Brush::Wood => "a wood: only what has a trunk seeds here",
            Brush::Low => "a meadow: everything but trees",
            Brush::Bare => "a clearing, a yard, a road",
            Brush::Sky => "not land at all; the sky colors are read from here",
        }
    }

    /// What it paints with, and what a press with that color is read back as.
    /// Chosen to be legible over a photograph rather than to look like the
    /// finished map: this is a legend, and it should not be mistaken for the
    /// thing it describes.
    pub fn color(self) -> u32 {
        match self {
            Brush::Clear => crate::util::EMPTY_COLOR,
            Brush::Water => crate::util::pack_rgba(58, 132, 214, 255),
            Brush::Rock => crate::util::pack_rgba(146, 146, 152, 255),
            Brush::Cliff => crate::util::pack_rgba(86, 78, 92, 255),
            Brush::Grass => crate::util::pack_rgba(96, 172, 84, 255),
            Brush::Sand => crate::util::pack_rgba(224, 202, 138, 255),
            Brush::Wood => crate::util::pack_rgba(38, 108, 62, 255),
            Brush::Low => crate::util::pack_rgba(156, 196, 96, 255),
            Brush::Bare => crate::util::pack_rgba(126, 106, 84, 255),
            Brush::Sky => crate::util::pack_rgba(126, 186, 232, 255),
        }
    }

    pub fn from_color(v: u32) -> Brush {
        BRUSHES.iter().copied().find(|b| b.color() == v).unwrap_or(Brush::Clear)
    }

    /// What a layer is, guessed from the name of its file: one exported as
    /// `water.png` or `03 trees.png` is what it says it is. The words are
    /// tried in a fixed order rather than the order they appear, so a "rock
    /// face" is a face and not a rock, and a name that says nothing anybody
    /// recognizes is `Clear`, which the page reads as "not said yet".
    pub fn guess(name: &str) -> Brush {
        const WORDS: [(&str, Brush); 24] = [
            ("cliff", Brush::Cliff),
            ("face", Brush::Cliff),
            ("water", Brush::Water),
            ("sea", Brush::Water),
            ("lake", Brush::Water),
            ("river", Brush::Water),
            ("ocean", Brush::Water),
            ("rock", Brush::Rock),
            ("stone", Brush::Rock),
            ("grass", Brush::Grass),
            ("sand", Brush::Sand),
            ("beach", Brush::Sand),
            ("shore", Brush::Sand),
            ("wood", Brush::Wood),
            ("tree", Brush::Wood),
            ("forest", Brush::Wood),
            ("meadow", Brush::Low),
            ("shrub", Brush::Low),
            ("low", Brush::Low),
            ("bare", Brush::Bare),
            ("nothing", Brush::Bare),
            ("road", Brush::Bare),
            ("clearing", Brush::Bare),
            ("sky", Brush::Sky),
        ];
        let name = name.to_ascii_lowercase();
        // Whole words, so "yellow" is not low; a word may carry a plural or
        // an ending, so "trees" and "rocks" still count.
        let words: Vec<&str> = name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        WORDS
            .iter()
            .find(|(word, _)| words.iter().any(|w| w.starts_with(word)))
            .map(|(_, brush)| *brush)
            .unwrap_or(Brush::Clear)
    }

    /// Its place in the list by name, which is how the page's own list of
    /// layers reads a choice back.
    pub fn from_key(key: &str) -> Brush {
        BRUSHES
            .iter()
            .copied()
            .find(|b| crate::util::slug(b.label()) == key)
            .unwrap_or(Brush::Clear)
    }

    pub fn key(self) -> String {
        crate::util::slug(self.label())
    }

    /// The ground it makes, for the brushes that are about the ground.
    pub fn ground(self) -> Option<Cell> {
        match self {
            Brush::Water => Some(Cell::Water),
            Brush::Rock => Some(Cell::Rock),
            Brush::Cliff => Some(Cell::Cliff),
            Brush::Grass => Some(Cell::Grass),
            Brush::Sand => Some(Cell::Sand),
            _ => None,
        }
    }

    /// What may take root, for the brushes that are about growth. The eraser
    /// answers here too, with the zone that says nothing.
    pub fn zone(self) -> Option<Zone> {
        match self {
            Brush::Wood => Some(Zone::Wood),
            Brush::Low => Some(Zone::Low),
            Brush::Bare => Some(Zone::Bare),
            Brush::Clear => Some(Zone::Any),
            _ => None,
        }
    }

    /// The brush a zone drawn on the map reads back as, which is what the
    /// editor shows over the ground.
    pub fn of_zone(zone: Zone) -> Brush {
        match zone {
            Zone::Wood => Brush::Wood,
            Zone::Low => Brush::Low,
            Zone::Bare => Brush::Bare,
            Zone::Any => Brush::Clear,
        }
    }

    /// The brush a cell of ground reads back as.
    pub fn of_ground(kind: Cell) -> Brush {
        match kind {
            Cell::Water => Brush::Water,
            Cell::Rock => Brush::Rock,
            Cell::Cliff => Brush::Cliff,
            Cell::Sand => Brush::Sand,
            Cell::Grass => Brush::Grass,
        }
    }
}

/// Whether two colors are within the threshold of each other, as a fraction
/// of the furthest apart two colors can be. What the fill by color on the map
/// page decides by.
pub fn near(a: u32, b: u32, threshold: f64) -> bool {
    let (a, b) = (unpack_rgba(a), unpack_rgba(b));
    let d = ((a.r as f64 - b.r as f64).powi(2)
        + (a.g as f64 - b.g as f64).powi(2)
        + (a.b as f64 - b.b as f64).powi(2))
    .sqrt();
    d / (255.0 * 3.0f64.sqrt()) <= threshold.clamp(0.0, 1.0)
}

/// How far a pixel's three channels may be apart and still read as gray. A
/// mask drawn with a soft brush or saved through a lossy format is a few
/// counts off neutral here and there, and that is still a mask.
const GRAY_SPREAD: u8 = 12;

/// Whether a pixel is the light half of a mask.
fn lit(c: crate::util::Rgba) -> bool {
    c.r as u32 * 299 + c.g as u32 * 587 + c.b as u32 * 114 >= 128 * 1000
}

/// Where a layer says its thing is, one flag a pixel, and whether it had to
/// be read by brightness. A layer out of a drawing program is clear wherever
/// nothing was drawn, so a pixel that is not clear is the mark.
///
/// A layer with no clear pixel in it may be either of two things, and the
/// colors are what tell them apart. A mask is gray and has a light half and a
/// dark half: it says where its thing is by brightness, and there are no
/// colors in it worth keeping. A picture that covers the whole map - a
/// coastline drawn edge to edge, a photograph - is neither, and reading it as
/// a mask is how a map made from one came out as nothing but the base ground:
/// its dark half said "nothing here" and its colors were dropped with the
/// mask they were mistaken for. So a fully opaque layer is read by brightness
/// only when it looks like a mask, and is otherwise a drawing that covers
/// everything, which is what it is.
pub fn layer_mask(w: i32, h: i32, px: &[u32]) -> (Vec<bool>, bool) {
    let n = (w.max(0) * h.max(0)) as usize;
    let cut = crate::civ::sprites::ALPHA_CUT;
    let pixel = |i: usize| unpack_rgba(px.get(i).copied().unwrap_or(0));
    let (mut opaque, mut gray, mut light, mut dark) = (true, true, false, false);
    for i in 0..n {
        let c = pixel(i);
        if c.a < cut {
            opaque = false;
            break;
        }
        if c.r.max(c.g).max(c.b) - c.r.min(c.g).min(c.b) > GRAY_SPREAD {
            gray = false;
            break;
        }
        if lit(c) {
            light = true;
        } else {
            dark = true;
        }
    }
    let by_light = opaque && gray && light && dark;
    let on = (0..n)
        .map(|i| {
            let c = pixel(i);
            if by_light {
                lit(c)
            } else {
                c.a >= cut
            }
        })
        .collect();
    (on, by_light)
}

/// One layer as it is read: what it is, how large it was drawn, and where it
/// has something.
pub struct LayerMask<'a> {
    pub brush: Brush,
    pub w: i32,
    pub h: i32,
    pub on: &'a [bool],
}

/// The map a set of layers makes: the ground of every cell, the zone of
/// every cell and which cells are marked sky, one byte a cell each. Three
/// grids rather than one, because they are three questions about a cell and
/// a layer of trees over a layer of sand answers two of them. With them, the
/// picture the map is drawn as, if there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapCells {
    pub cols: i32,
    pub rows: i32,
    pub ground: Vec<u8>,
    pub zone: Vec<u8>,
    pub sky: Vec<u8>,
    pub art: Option<MapArt>,
}

/// One layer's pixels as they were dropped, for flattening into the map's
/// picture.
pub struct LayerArt<'a> {
    pub w: i32,
    pub h: i32,
    pub px: &'a [u32],
    /// Read light against dark: a mask rather than a drawing, with no colors
    /// worth keeping.
    pub by_light: bool,
}

/// The layers flattened into one picture. The first drawing sets the size and
/// each later one goes over the earlier where it has something, the way the
/// layers stack in the program they came out of; a layer read as a mask is
/// left out. Nothing if no layer was a drawing, or none of them shows.
pub fn flatten_layers(layers: &[LayerArt]) -> Option<MapArt> {
    let mut out: Option<MapArt> = None;
    for layer in layers.iter().filter(|l| !l.by_light && l.w > 0 && l.h > 0) {
        let art = out.get_or_insert_with(|| {
            MapArt::new(layer.w, layer.h, vec![0; (layer.w * layer.h) as usize])
        });
        for y in 0..art.h {
            let sy = (y as i64 * layer.h as i64 / art.h as i64).clamp(0, layer.h as i64 - 1);
            for x in 0..art.w {
                let sx = (x as i64 * layer.w as i64 / art.w as i64).clamp(0, layer.w as i64 - 1);
                let v = layer.px.get((sy * layer.w as i64 + sx) as usize).copied().unwrap_or(0);
                if unpack_rgba(v).a >= ALPHA_CUT {
                    art.px[(y * art.w + x) as usize] = v;
                }
            }
        }
    }
    out.filter(|art| art.shows())
}

/// The smallest map a drawing is allowed to make. There is no ceiling on the
/// size - a drawing is worth however many cells it was drawn with - but there
/// is a floor, because a town cannot be founded on a map of nine cells and a
/// picture dropped by mistake should not be the thing that finds that out.
pub const MIN_COLS: i32 = 16;
pub const MIN_ROWS: i32 = 8;

/// The most of a drawing that is ever taken for sky. A drawing that is sky
/// all the way down says nothing about any land, and cutting all of it off
/// would leave no map at all, so what is left below is land whatever it looks
/// like.
const MOST_SKY: f64 = 0.9;

/// The map a drawing makes: how many cells across, and how many deep.
///
/// Across is one cell per `px` of its pixels, which is the number on the
/// page. Deep is not, and this is the whole of why a map read in used to come
/// out squashed: the ground is a plane seen at an angle, a row of it drawn
/// `depth_px` tall where a column is `cell_px` wide, so a drawing laid cell
/// for cell is drawn five eighths as tall as it was drawn. Taking a row per
/// `px * depth_px / cell_px` pixels instead - more rows than pixels, at every
/// angle the ground is ever seen from - is what puts the drawing on the
/// screen the shape somebody drew it.
///
/// The cells are the map rather than the drawing, so nothing is lost by
/// there being more of them than there were pixels: each layer is stretched
/// over the map it makes, and a cell is the nearest pixel of it.
pub fn map_cells(w: i32, h: i32, px: i32, cell_px: i32, depth_px: i32) -> (i32, i32) {
    let px = px.max(1) as f64;
    let tilt = cell_px.max(1) as f64 / depth_px.max(1) as f64;
    let cols = (w.max(0) as f64 / px).floor() as i32;
    let rows = ((h.max(0) as f64 / px) * tilt).round() as i32;
    (cols.max(MIN_COLS), rows.max(MIN_ROWS))
}

/// How much of a drawing is sky, as a share of its height: the run of rows
/// from the top that are more sky than not.
///
/// A drawing of a place has a sky in it and the sky is not land. Laid on the
/// ground plane with everything else it becomes a band of ground painted like
/// a sky across the back of the map, which crushes the land into what is left
/// below it - and the settlement has a sky of its own to draw, right above
/// where that band ends. So the band is cut off and the world's sky stands
/// where it was.
///
/// A run from the top rather than every row that has sky in it, because a
/// drawing has sky between the trees as well as over them, and only what
/// reaches the top of the picture is a sky anybody stands under. Mostly sky
/// rather than wholly, because a horizon is not a straight line: a row with a
/// hill in it is still a row of sky.
pub fn sky_band(layers: &[LayerMask]) -> f64 {
    let mut share: f64 = 0.0;
    for layer in layers.iter().filter(|l| l.brush == Brush::Sky && l.w > 0 && l.h > 0) {
        let mut rows = 0;
        for y in 0..layer.h {
            let on = (0..layer.w)
                .filter(|x| layer.on.get((y * layer.w + x) as usize).copied().unwrap_or(false))
                .count();
            if on * 2 <= layer.w as usize {
                break;
            }
            rows += 1;
        }
        share = share.max(rows as f64 / layer.h as f64);
    }
    share.clamp(0.0, MOST_SKY)
}

/// How many rows off the top of a picture `h` tall that share is, never all
/// of them.
pub fn sky_rows(h: i32, share: f64) -> i32 {
    ((h as f64 * share.clamp(0.0, 1.0)).round() as i32).clamp(0, (h - 1).max(0))
}

/// A picture with the top `cut` rows taken off it: the land under a sky.
/// Whatever the picture is made of - where a layer has something, or the
/// pixels themselves - the cut is the same rows of it.
pub fn below<T: Copy>(w: i32, h: i32, cut: i32, data: &[T]) -> Vec<T> {
    let cut = cut.clamp(0, h.max(0));
    let from = (cut.max(0) * w.max(0)) as usize;
    let to = ((w.max(0) * h.max(0)) as usize).min(data.len());
    data.get(from..to).map(<[T]>::to_vec).unwrap_or_default()
}

/// The colors of a sky that was drawn: the top of the band and the row above
/// the horizon, each averaged across the width so that one bird does not
/// become the sky. They are the two ends of the gradient the settlement draws
/// its own sky with, which is how a drawing's weather arrives with its land.
/// Nothing if there is no band, or nothing opaque in it.
pub fn sky_colors(w: i32, h: i32, px: &[u32], cut: i32) -> Option<(u32, u32)> {
    if w <= 0 || cut <= 0 || cut > h {
        return None;
    }
    let row = |y: i32| -> Option<u32> {
        let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
        for x in 0..w {
            let c = unpack_rgba(px.get((y * w + x) as usize).copied().unwrap_or(0));
            if c.a < ALPHA_CUT {
                continue;
            }
            r += c.r as u32;
            g += c.g as u32;
            b += c.b as u32;
            n += 1;
        }
        (n > 0).then(|| {
            crate::util::pack_rgba((r / n) as i32, (g / n) as i32, (b / n) as i32, 255)
        })
    };
    Some((row(0)?, row(cut - 1)?))
}

/// Every cell of a map, read out of a set of layers. Each layer is stretched
/// corner to corner over the map, so a set exported from one drawing lands
/// cell for cell and a stray one of another size still lands somewhere
/// sensible. Where a layer has something, the cell is what the layer is: a
/// ground layer sets the ground, a zone layer the zone, a sky layer the mark,
/// and two layers answering the same question are read in order with the
/// later one winning. A layer set to `Clear` says nothing. Every cell no
/// ground layer covers is `base`.
pub fn read_layers(layers: &[LayerMask], cols: i32, rows: i32, base: Cell) -> MapCells {
    let n = (cols.max(0) * rows.max(0)) as usize;
    let mut out = MapCells {
        cols,
        rows,
        ground: vec![base as u8; n],
        zone: vec![Zone::Any as u8; n],
        sky: vec![0; n],
        art: None,
    };
    for layer in layers {
        if layer.brush == Brush::Clear || layer.w <= 0 || layer.h <= 0 {
            continue;
        }
        let ground = layer.brush.ground();
        let zone = layer.brush.zone();
        for r in 0..rows {
            let sy = (((r as f64 + 0.5) / rows as f64) * layer.h as f64).floor() as i32;
            let sy = sy.clamp(0, layer.h - 1);
            for c in 0..cols {
                let sx = (((c as f64 + 0.5) / cols as f64) * layer.w as f64).floor() as i32;
                let sx = sx.clamp(0, layer.w - 1);
                if !layer.on.get((sy * layer.w + sx) as usize).copied().unwrap_or(false) {
                    continue;
                }
                let i = (r * cols + c) as usize;
                if let Some(kind) = ground {
                    out.ground[i] = kind as u8;
                } else if let Some(zone) = zone {
                    out.zone[i] = zone as u8;
                } else if layer.brush == Brush::Sky {
                    out.sky[i] = 1;
                }
            }
        }
    }
    out
}

/// Lays a read map over a settlement's, one cell at a time, and hands it the
/// picture to be drawn as. Meant for a settlement that has been made and not
/// yet founded, so the wilderness grows on the painted ground rather than
/// being flattened by it afterwards. The sky marks are not laid: they belong
/// to the page, not the map.
pub fn lay_cells(sim: &mut crate::civ::settlement::Settlement, cells: &MapCells) {
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    if (cols, rows) != (cells.cols, cells.rows) {
        return;
    }
    sim.set_art(cells.art.clone());
    for r in 0..rows {
        for c in 0..cols {
            let i = (r * cols + c) as usize;
            if let Some(&kind) = cells.ground.get(i) {
                sim.paint_cell(c, r, Cell::from_u8(kind));
            }
            match cells.zone.get(i).copied().map(Zone::from_u8) {
                Some(Zone::Any) | None => {}
                Some(zone) => sim.terrain.set_zone(c, r, zone),
            }
        }
    }
    sim.rebuild_plant_index();
    sim.sync_zones();
}
