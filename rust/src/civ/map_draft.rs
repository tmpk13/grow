//! A map drawn by hand, over a picture.
//!
//! The draft is a grid of brushes, one per cell, and nothing else. It says
//! what the land is - water here, a rock face there, a wood, a meadow, sky -
//! and it is applied to whatever map is running by stretching it over that
//! map corner to corner, the same way a dropped landscape is read.
//!
//! The picture somebody traces is not in here. A photograph is megabytes and
//! the draft is a few kilobytes of runs; the picture lives with the window for
//! as long as the page is open, and what survives a reload is the drawing.
//!
//! Nothing in here touches a running settlement on its own. It is a drawing
//! until somebody presses Apply, which is the whole reason it can be kept in
//! the project beside the settings rather than in the settlement beside the
//! ground.

use serde::{Deserialize, Serialize};

use crate::civ::terrain::Cell;
use crate::world::Zone;

/// What one cell of a draft is marked as.
///
/// Two of these are ground, two are what may grow, and one is not land at all.
/// They are one list rather than three menus for the same reason the picture
/// tool's are: the questions are about the same cell, and somebody painting a
/// map is answering whichever one the cell needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Brush {
    /// Nothing said. Applying a draft leaves these cells exactly as they are,
    /// which is what makes a half painted draft useful.
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
    /// Not land. Whatever is painted sky is where the sky colors are read
    /// from, and no ground under it is touched.
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
    pub fn from_u8(v: u8) -> Brush {
        BRUSHES.get(v as usize).copied().unwrap_or(Brush::Clear)
    }

    pub fn id(self) -> u8 {
        BRUSHES.iter().position(|b| *b == self).unwrap_or(0) as u8
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
            Brush::Clear => "the map keeps whatever is already there",
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

    /// What it is drawn as while it is being painted, and what a press with
    /// that color on the stage is read back as. Chosen to be legible over a
    /// photograph rather than to look like the finished map: this is a
    /// drawing, and it should not be mistaken for the thing it describes.
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

    /// What may take root, for the brushes that are about growth.
    pub fn zone(self) -> Option<Zone> {
        match self {
            Brush::Wood => Some(Zone::Wood),
            Brush::Low => Some(Zone::Low),
            Brush::Bare => Some(Zone::Bare),
            _ => None,
        }
    }
}

/// The drawing. Sized in its own cells rather than the map's, so a draft
/// outlives the map it was made for and can be applied to a larger one.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MapDraft {
    pub cols: i32,
    pub rows: i32,
    /// One brush id per cell, row major. Stored as runs, because a map is
    /// mostly the same answer over and over and a plain list of eight
    /// thousand numbers is most of the project file.
    #[serde(with = "runs")]
    pub paint: Vec<u8>,
}

/// How wide and tall a fresh draft is. The same shape as the settlement's own
/// default map, so the first thing somebody paints lines up with the map they
/// are painting it for.
pub const DEFAULT_COLS: i32 = 128;
pub const DEFAULT_ROWS: i32 = 64;

impl Default for MapDraft {
    fn default() -> Self {
        MapDraft { cols: DEFAULT_COLS, rows: DEFAULT_ROWS, paint: Vec::new() }
    }
}

impl MapDraft {
    /// How many cells the grid holds. Not `len`, and the question below is not
    /// `is_empty`: a draft with cells in it can still have nothing said on it,
    /// and the two would read as each other's opposite.
    pub fn cells(&self) -> usize {
        (self.cols.max(0) * self.rows.max(0)) as usize
    }

    pub fn nothing_painted(&self) -> bool {
        self.paint.iter().all(|&v| v == 0)
    }

    /// The grid, grown to its own size if it has not been painted yet. Every
    /// read and write goes through the size the header says, so a draft
    /// resized between sessions never reads off the end of an old buffer.
    pub fn ensure(&mut self) {
        let n = self.cells();
        if self.paint.len() != n {
            self.paint.resize(n, 0);
        }
    }

    pub fn at(&self, col: i32, row: i32) -> Brush {
        if col < 0 || row < 0 || col >= self.cols || row >= self.rows {
            return Brush::Clear;
        }
        match self.paint.get((row * self.cols + col) as usize) {
            Some(&v) => Brush::from_u8(v),
            None => Brush::Clear,
        }
    }

    pub fn set(&mut self, col: i32, row: i32, brush: Brush) {
        if col < 0 || row < 0 || col >= self.cols || row >= self.rows {
            return;
        }
        self.ensure();
        let i = (row * self.cols + col) as usize;
        if let Some(slot) = self.paint.get_mut(i) {
            *slot = brush.id();
        }
    }

    /// Which cell of the draft covers a cell of a map this size. The draft is
    /// stretched corner to corner, which is what "used as the map" means.
    pub fn cell_for(&self, cols: i32, rows: i32, col: i32, row: i32) -> (i32, i32) {
        let x = ((col as f64 + 0.5) / cols.max(1) as f64 * self.cols as f64).floor() as i32;
        let y = ((row as f64 + 0.5) / rows.max(1) as f64 * self.rows as f64).floor() as i32;
        (x.clamp(0, (self.cols - 1).max(0)), y.clamp(0, (self.rows - 1).max(0)))
    }

    /// How many cells of each brush, for the readout under the picture.
    pub fn tally(&self) -> Vec<(Brush, usize)> {
        let mut out = Vec::new();
        for brush in BRUSHES {
            if brush == Brush::Clear {
                continue;
            }
            let n = self.paint.iter().filter(|&&v| Brush::from_u8(v) == brush).count();
            if n > 0 {
                out.push((brush, n));
            }
        }
        out
    }
}

/// The paint grid as the same run encoding a saved settlement uses for its
/// cell kinds, so a draft costs a few hundred characters in the project rather
/// than a few tens of thousands.
mod runs {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(px: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::civ::save::bytes_rle(px))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(crate::civ::save::bytes_from_rle(&raw))
    }
}
