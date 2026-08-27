//! Finding a way across the map.
//!
//! The settlement used to breadth-first the whole grid for every walk, and
//! clear an array the size of the map before each one. That is fine on a
//! hundred cells of land and hopeless on a hundred thousand: the clear alone
//! costs more than the search, and a failed search touches every cell there is.
//!
//! This is A* over the same eight neighbors with three things that make it
//! scale: visited marks are generation stamped rather than cleared, the
//! frontier is a heap ordered by an octile heuristic so a search reaches the
//! goal without fanning out over the whole map, and every search is capped so
//! an unreachable target costs a bounded amount rather than the whole grid.
//!
//! What a cell costs to step onto is the caller's to say, from the base step
//! for the direction. Less for ground that has been walked over, which is why
//! traffic wears into roads that people then prefer; far more for water, which
//! is why somebody swims a river only when walking round it would be much
//! further.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Cost of one step, in tenths, so the diagonal stays an integer.
const STEP: i32 = 10;
const DIAGONAL: i32 = 14;

pub const NEIGHBORS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

#[derive(Clone, Copy, PartialEq, Eq)]
struct Node {
    /// Estimated total cost through this cell. Negated ordering below turns
    /// the max-heap into the min-heap A* wants.
    f: i32,
    cell: i32,
}

impl Ord for Node {
    fn cmp(&self, other: &Node) -> Ordering {
        // Ties break on the cell index so the same map and the same request
        // always expand in the same order.
        other.f.cmp(&self.f).then(other.cell.cmp(&self.cell))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Node) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Search scratch sized to one map. Held by the settlement and taken out for
/// the duration of a search, because the passability test borrows the map.
pub struct PathGrid {
    cols: i32,
    rows: i32,
    stamp: Vec<u32>,
    g: Vec<i32>,
    from: Vec<i32>,
    gen: u32,
    open: BinaryHeap<Node>,
}

impl Default for PathGrid {
    fn default() -> Self {
        PathGrid::new(0, 0)
    }
}

impl PathGrid {
    pub fn new(cols: i32, rows: i32) -> Self {
        let n = (cols.max(0) * rows.max(0)) as usize;
        PathGrid {
            cols,
            rows,
            stamp: vec![0; n],
            g: vec![0; n],
            from: vec![-1; n],
            gen: 0,
            open: BinaryHeap::new(),
        }
    }

    pub fn resize(&mut self, cols: i32, rows: i32) {
        *self = PathGrid::new(cols, rows);
    }

    pub fn matches(&self, cols: i32, rows: i32) -> bool {
        self.cols == cols && self.rows == rows
    }

    fn in_bounds(&self, c: i32, r: i32) -> bool {
        c >= 0 && c < self.cols && r >= 0 && r < self.rows
    }

    fn idx(&self, c: i32, r: i32) -> usize {
        (r * self.cols + c) as usize
    }

    /// Octile distance: the exact cost of an unobstructed run, which is what
    /// keeps A* from wandering while staying admissible.
    fn heuristic(&self, a: i32, b: i32) -> i32 {
        let (ac, ar) = (a % self.cols, a / self.cols);
        let (bc, br) = (b % self.cols, b / self.cols);
        let dx = (ac - bc).abs();
        let dy = (ar - br).abs();
        STEP * (dx + dy) + (DIAGONAL - 2 * STEP) * dx.min(dy)
    }

    /// A route from start to goal, or None. `passable` decides which cells may
    /// be crossed; the goal itself is always allowed, so somebody can walk up
    /// to a door. `wear` returns how worn a cell is, in the same units the
    /// traffic map uses, and shaves up to a third off the cost of crossing it.
    pub fn find(
        &mut self,
        start: (i32, i32),
        goal: (i32, i32),
        budget: usize,
        passable: impl Fn(i32, i32) -> bool,
        cost: impl Fn(usize, i32) -> i32,
    ) -> Option<Vec<(i32, i32)>> {
        if !self.in_bounds(start.0, start.1) || !self.in_bounds(goal.0, goal.1) {
            return None;
        }
        if start == goal {
            return Some(Vec::new());
        }
        let start_i = self.idx(start.0, start.1) as i32;
        let goal_i = self.idx(goal.0, goal.1) as i32;

        self.gen = self.gen.wrapping_add(1);
        if self.gen == 0 {
            // Wrapped: the only moment the stamps have to be cleared.
            self.stamp.fill(0);
            self.gen = 1;
        }
        let gen = self.gen;
        self.open.clear();
        self.stamp[start_i as usize] = gen;
        self.g[start_i as usize] = 0;
        self.from[start_i as usize] = start_i;
        self.open.push(Node { f: self.heuristic(start_i, goal_i), cell: start_i });

        let mut expanded = 0usize;
        let mut found = false;
        while let Some(node) = self.open.pop() {
            if node.cell == goal_i {
                found = true;
                break;
            }
            expanded += 1;
            if expanded > budget {
                break;
            }
            let cur = node.cell;
            let cost_here = self.g[cur as usize];
            let cc = cur % self.cols;
            let cr = cur / self.cols;
            for (dx, dy) in NEIGHBORS {
                let nc = cc + dx;
                let nr = cr + dy;
                if !self.in_bounds(nc, nr) {
                    continue;
                }
                let ni = self.idx(nc, nr) as i32;
                if ni != goal_i && !passable(nc, nr) {
                    continue;
                }
                // Corners are not cut: a diagonal step needs both of the
                // orthogonal cells beside it, or people walk through the joint
                // of two walls and across the tip of a lake.
                if dx != 0 && dy != 0 && !(passable(cc + dx, cr) && passable(cc, cr + dy)) {
                    continue;
                }
                let base = if dx != 0 && dy != 0 { DIAGONAL } else { STEP };
                let step = cost(ni as usize, base);
                let next = cost_here + step.max(1);
                if self.stamp[ni as usize] == gen && self.g[ni as usize] <= next {
                    continue;
                }
                self.stamp[ni as usize] = gen;
                self.g[ni as usize] = next;
                self.from[ni as usize] = cur;
                self.open.push(Node { f: next + self.heuristic(ni, goal_i), cell: ni });
            }
        }
        if !found {
            return None;
        }

        let mut path = Vec::new();
        let mut cur = goal_i;
        while cur != start_i {
            path.push((cur % self.cols, cur / self.cols));
            cur = self.from[cur as usize];
            if path.len() > (self.cols * self.rows) as usize {
                return None;
            }
        }
        path.reverse();
        Some(path)
    }
}
