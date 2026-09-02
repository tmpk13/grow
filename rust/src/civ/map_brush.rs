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

use crate::civ::terrain::Cell;
use crate::world::Zone;

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

    /// The nearest brush to a color out of a picture, for reading a drawing of
    /// a map in as a map. Everything is nearest to something, so this always
    /// answers; `Clear` and `Sky` are left out of the running because neither
    /// is a kind of ground a cell could be turned into.
    pub fn nearest(v: u32) -> Brush {
        let want = crate::util::unpack_rgba(v);
        let mut best = (i32::MAX, Brush::Grass);
        for brush in BRUSHES {
            if brush == Brush::Clear || brush == Brush::Sky {
                continue;
            }
            let c = crate::util::unpack_rgba(brush.color());
            let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
            let far = d(want.r, c.r) + d(want.g, c.g) + d(want.b, c.b);
            if far < best.0 {
                best = (far, brush);
            }
        }
        best.1
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

/// Every cell of a map, read out of a picture. The picture is stretched corner
/// to corner over the map it is about to become, which at the scale the map
/// editor offers is one cell per block of pixels, and every color in it is
/// read as the nearest thing in the legend.
pub fn read_picture(image: &(i32, i32, Vec<u32>), cols: i32, rows: i32) -> Vec<u8> {
    let (iw, ih, px) = image;
    let mut out = vec![0u8; (cols.max(0) * rows.max(0)) as usize];
    for r in 0..rows {
        for c in 0..cols {
            let sx = (((c as f64 + 0.5) / cols as f64) * *iw as f64).floor() as i32;
            let sy = (((r as f64 + 0.5) / rows as f64) * *ih as f64).floor() as i32;
            let v = px
                .get((sy.clamp(0, ih - 1) * iw + sx.clamp(0, iw - 1)) as usize)
                .copied()
                .unwrap_or(0);
            out[(r * cols + c) as usize] = Brush::nearest(v).id();
        }
    }
    out
}

/// Lays a read picture over a map, one cell at a time. Meant for a settlement
/// that has been made and not yet founded, so the wilderness grows on the
/// painted ground rather than being flattened by it afterwards.
pub fn lay_cells(sim: &mut crate::civ::settlement::Settlement, cells: &[u8]) {
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    for r in 0..rows {
        for c in 0..cols {
            let brush = match cells.get((r * cols + c) as usize) {
                Some(&id) => Brush::from_u8(id),
                None => continue,
            };
            if let Some(kind) = brush.ground() {
                sim.paint_cell(c, r, kind);
            }
            match brush.zone() {
                Some(Zone::Any) | None => {}
                Some(zone) => sim.terrain.set_zone(c, r, zone),
            }
        }
    }
    sim.rebuild_plant_index();
    sim.sync_zones();
}
