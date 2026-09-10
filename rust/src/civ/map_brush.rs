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
//! that carried every kind at once did.

use crate::civ::terrain::Cell;
use crate::util::unpack_rgba;
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

/// Where a layer says its thing is, one flag a pixel, and whether it had to
/// be read by brightness. A layer out of a drawing program is clear wherever
/// nothing was drawn, so a pixel that is not clear is the mark. One with no
/// clear pixel in it at all was drawn as a mask instead, light where the
/// thing is and dark where it is not, and is read that way.
pub fn layer_mask(w: i32, h: i32, px: &[u32]) -> (Vec<bool>, bool) {
    let n = (w.max(0) * h.max(0)) as usize;
    let cut = crate::civ::sprites::ALPHA_CUT;
    let pixel = |i: usize| unpack_rgba(px.get(i).copied().unwrap_or(0));
    let by_light = (0..n).all(|i| pixel(i).a >= cut);
    let on = (0..n)
        .map(|i| {
            let c = pixel(i);
            if by_light {
                c.r as u32 * 299 + c.g as u32 * 587 + c.b as u32 * 114 >= 128 * 1000
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
/// a layer of trees over a layer of sand answers two of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapCells {
    pub cols: i32,
    pub rows: i32,
    pub ground: Vec<u8>,
    pub zone: Vec<u8>,
    pub sky: Vec<u8>,
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

/// Lays a read map over a settlement's, one cell at a time. Meant for a
/// settlement that has been made and not yet founded, so the wilderness grows
/// on the painted ground rather than being flattened by it afterwards. The
/// sky marks are not laid: they belong to the page, not the map.
pub fn lay_cells(sim: &mut crate::civ::settlement::Settlement, cells: &MapCells) {
    let (cols, rows) = (sim.world().cols, sim.world().rows);
    if (cols, rows) != (cells.cols, cells.rows) {
        return;
    }
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
