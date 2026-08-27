//! The world grid, a 2.5D area.
//!
//! The grid is a ground plane seen at an angle: columns run left to right (x)
//! and rows run from the back of the area to the front (depth). A cell is drawn
//! `cell_px` wide and `depth_px` tall, so a row of depth is foreshortened, and
//! plants stand up out of their cell toward the top of the screen.
//!
//!   screen x = col * cell_px
//!   screen y = sky_px + row * depth_px      (row 0 is the far edge)
//!
//! Occupancy is tracked per size class layer, which is what allows several
//! items to occupy one cell (ground cover plus a tree) while still forbidding
//! two items of the same class in one place.

use serde::{Deserialize, Serialize};

use crate::species::LAYER_COUNT;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorldConfig {
    pub cols: i32,
    pub rows: i32,
    pub cell_px: i32,
    pub depth_px: i32,
    pub sky_px: i32,
    pub sky_top: String,
    pub sky_bottom: String,
    pub soil_sampler: String,
    pub depth_fade: f64,
    pub shadows: bool,
}

impl Default for WorldConfig {
    fn default() -> Self {
        WorldConfig {
            cols: 64,
            rows: 24,
            cell_px: 8,
            depth_px: 5,
            sky_px: 150,
            sky_top: "#101a26".into(),
            sky_bottom: "#33424a".into(),
            soil_sampler: "mat-soil".into(),
            depth_fade: 0.16,
            shadows: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Support {
    pub col: i32,
    pub row: i32,
    pub owner: i32,
    pub layer: usize,
}

#[derive(Clone, Debug, Default)]
pub struct World {
    pub cols: i32,
    pub rows: i32,
    pub cell_px: i32,
    pub depth_px: i32,
    pub sky_px: i32,
    pub depth_ratio: f64,
    pub px_w: i32,
    pub ground_px: i32,
    pub front_px: i32,
    pub px_h: i32,
    pub layers: Vec<Vec<i32>>,
}

impl World {
    pub fn new(cfg: &WorldConfig) -> Self {
        let mut w = World::default();
        w.configure(cfg);
        w
    }

    pub fn configure(&mut self, cfg: &WorldConfig) {
        self.cols = cfg.cols.max(4);
        self.rows = cfg.rows.max(2);
        self.cell_px = cfg.cell_px.max(1);
        self.depth_px = cfg.depth_px.max(1);
        self.sky_px = cfg.sky_px.max(0);
        self.depth_ratio = self.depth_px as f64 / self.cell_px as f64;
        self.px_w = self.cols * self.cell_px;
        self.ground_px = self.rows * self.depth_px;
        // Ground past the front row, wide enough that a mat centered on the
        // nearest row is not clipped by the bottom of the buffer.
        self.front_px = self.cell_px.max(self.depth_px * 3);
        self.px_h = self.sky_px + self.ground_px + self.front_px;
        self.layers = (0..LAYER_COUNT)
            .map(|_| vec![0i32; (self.cols * self.rows) as usize])
            .collect();
    }

    pub fn clear(&mut self) {
        for layer in &mut self.layers {
            layer.fill(0);
        }
    }

    pub fn in_bounds(&self, cx: i32, cy: i32) -> bool {
        cx >= 0 && cx < self.cols && cy >= 0 && cy < self.rows
    }

    pub fn idx(&self, cx: i32, cy: i32) -> usize {
        (cy * self.cols + cx) as usize
    }

    /// Screen position a plant rooted in this cell is anchored to: the middle
    /// of the cell on the ground plane.
    pub fn anchor_x(&self, cx: i32) -> i32 {
        cx * self.cell_px + self.cell_px / 2
    }

    pub fn anchor_y(&self, cy: i32) -> i32 {
        self.sky_px + cy * self.depth_px + self.depth_px / 2
    }

    pub fn occupant(&self, layer: usize, cx: i32, cy: i32) -> i32 {
        if !self.in_bounds(cx, cy) {
            return 0;
        }
        self.layers[layer][self.idx(cx, cy)]
    }

    /// Cells covered by a footprint of the given radius, as a disc on the
    /// ground plane, returned as flat indices.
    pub fn footprint(&self, cx: i32, cy: i32, radius_cells: i32, out: &mut Vec<usize>) {
        out.clear();
        let r = radius_cells.max(0);
        let rf = r as f64 + 0.35;
        let r2 = rf * rf;
        for y in cy - r..=cy + r {
            if y < 0 || y >= self.rows {
                continue;
            }
            for x in cx - r..=cx + r {
                if x < 0 || x >= self.cols {
                    continue;
                }
                let dx = (x - cx) as f64;
                let dy = (y - cy) as f64;
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                out.push((y * self.cols + x) as usize);
            }
        }
    }

    /// True when every cell is free or already owned by this instance.
    pub fn can_claim(&self, layer: usize, cells: &[usize], instance_id: i32) -> bool {
        let grid = &self.layers[layer];
        cells.iter().all(|&i| grid[i] == 0 || grid[i] == instance_id)
    }

    pub fn claim(&mut self, layer: usize, cells: &[usize], instance_id: i32) {
        let grid = &mut self.layers[layer];
        for &i in cells {
            grid[i] = instance_id;
        }
    }

    pub fn release(&mut self, layer: usize, cells: &[usize], instance_id: i32) {
        let grid = &mut self.layers[layer];
        for &i in cells {
            if grid[i] == instance_id {
                grid[i] = 0;
            }
        }
    }

    /// Spacing test over the area, same layer.
    pub fn has_neighbor_within(&self, layer: usize, cx: i32, cy: i32, spacing: i32) -> bool {
        if spacing <= 0 {
            return false;
        }
        let grid = &self.layers[layer];
        let r2 = (spacing * spacing) as f64;
        for y in cy - spacing..=cy + spacing {
            if y < 0 || y >= self.rows {
                continue;
            }
            for x in cx - spacing..=cx + spacing {
                if x < 0 || x >= self.cols {
                    continue;
                }
                let dx = (x - cx) as f64;
                let dy = (y - cy) as f64;
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                if grid[(y * self.cols + x) as usize] != 0 {
                    return true;
                }
            }
        }
        false
    }

    /// Nearest woody support for a climbing plant: searches the area outward
    /// and returns the closest occupied cell in one of the given layers.
    pub fn find_support(
        &self,
        cx: i32,
        cy: i32,
        search_cells: i32,
        support_layers: &[usize],
    ) -> Option<Support> {
        let mut best: Option<Support> = None;
        let mut best_dist = i32::MAX;
        for y in cy - search_cells..=cy + search_cells {
            if y < 0 || y >= self.rows {
                continue;
            }
            for x in cx - search_cells..=cx + search_cells {
                if x < 0 || x >= self.cols {
                    continue;
                }
                let dx = x - cx;
                let dy = y - cy;
                let d = dx * dx + dy * dy;
                if d > search_cells * search_cells || d >= best_dist {
                    continue;
                }
                for &layer in support_layers {
                    let owner = self.layers[layer][(y * self.cols + x) as usize];
                    if owner != 0 {
                        best = Some(Support { col: x, row: y, owner, layer });
                        best_dist = d;
                        break;
                    }
                }
            }
        }
        best
    }

    /// Occupancy bitmask per cell, used by the debug overlay.
    pub fn occupancy_at(&self, cx: i32, cy: i32) -> u32 {
        let mut mask = 0;
        let i = self.idx(cx, cy);
        for l in 0..LAYER_COUNT {
            if self.layers[l][i] != 0 {
                mask |= 1 << l;
            }
        }
        mask
    }
}
