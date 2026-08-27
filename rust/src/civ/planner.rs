//! The build planner.
//!
//! Nobody decides where the next building goes by hand: the planner scores
//! every unlocked building against what the store is short of, checks that the
//! settlement can plausibly pay for it, and then scores every legal cell for a
//! site. Weights for all of it live in the build config, so a settlement can be
//! pushed toward housing, industry or civic work by changing numbers.

use std::collections::HashMap;

use crate::civ::buildings::{
    building_by_id, scaled_cost, BuildingDef, Category, Job, Site, Structure, BUILDINGS,
};
use crate::civ::economy::stock_targets;
use crate::civ::resources::{stock_bulk, Res};
use crate::civ::settlement::Settlement;
use crate::state::State;
use crate::util::clamp;

pub fn can_place_at(sim: &Settlement, state: &State, def: &BuildingDef, col: i32, row: i32) -> bool {
    // A wall is a line, not a building: the gap the planner keeps around
    // everything else would leave a fence full of holes.
    let spacing = if def.structure.abuts() { 0 } else { state.civ.build.spacing };
    fits(sim, def, col, row, 0, spacing)
}

/// Whether a footprint may stand here.
///
/// `ignore` is a building whose own cells count as empty, which is what lets a
/// house be rebuilt as a manor over the ground it already stands on, and
/// `spacing` is the gap kept from other buildings; a rebuild passes zero,
/// because a house is allowed to grow into its own yard.
pub fn fits(
    sim: &Settlement,
    def: &BuildingDef,
    col: i32,
    row: i32,
    ignore: i32,
    spacing: i32,
) -> bool {
    let spacing = spacing.max(0);
    let taken = |c: i32, r: i32| {
        let id = sim.build_grid[sim.idx(c, r)];
        id != 0 && id != ignore
    };
    for r in row - spacing..row + def.h + spacing {
        for c in col - spacing..col + def.w + spacing {
            if !sim.in_bounds(c, r) {
                return false;
            }
            let in_footprint = c >= col && c < col + def.w && r >= row && r < row + def.h;
            if in_footprint {
                if !sim.terrain.is_buildable(c, r) {
                    return false;
                }
                if sim.terrain.deposit_at(c, r).is_some() {
                    return false;
                }
                if taken(c, r) {
                    return false;
                }
            } else if taken(c, r) {
                return false;
            }
        }
    }
    // A dock has to reach the water it is a dock for.
    if matches!(def.site, Some(Site::Shore)) && !shore_reach(sim, def, col, row) {
        return false;
    }
    // Somewhere to walk up to.
    let mut open = 0;
    for r in row - 1..=row + def.h {
        for c in col - 1..=col + def.w {
            let inside = c >= col && c < col + def.w && r >= row && r < row + def.h;
            if inside {
                continue;
            }
            let free = sim.in_bounds(c, r)
                && sim.terrain.kind[sim.idx(c, r)] != crate::civ::terrain::Cell::Water as u8
                && !taken(c, r);
            if free {
                open += 1;
            }
        }
    }
    open >= 2
}

/// How far a jetty may stand from navigable water, in cells.
fn shore_reach(sim: &Settlement, def: &BuildingDef, col: i32, row: i32) -> bool {
    let reach = if def.radius > 0.0 { def.radius as i32 } else { 3 };
    for r in row - reach..=row + def.h + reach {
        for c in col - reach..=col + def.w + reach {
            if sim.terrain.navigable(c, r) {
                return true;
            }
        }
    }
    false
}

pub fn site_score(sim: &Settlement, ci: usize, def: &BuildingDef, col: i32, row: i32) -> f64 {
    let colony = sim.colonies[ci].id;
    let mut score = 0.0;
    match def.site {
        Some(Site::Deposit(kind)) => {
            let radius = if def.radius > 0.0 { def.radius } else { 10.0 };
            let dep = match sim.terrain.find_deposit(kind, col, row, radius) {
                Some(d) => &sim.terrain.deposits[d],
                None => return f64::NEG_INFINITY,
            };
            let d = ((dep.col - col) as f64).hypot((dep.row - row) as f64);
            score += 12.0 - d;
        }
        Some(Site::Fertile) => {
            let mut fert = 0.0;
            let rad = if def.fields > 0 { def.fields } else { 2 };
            for r in row - rad..=row + rad {
                for c in col - rad..=col + rad {
                    fert += sim.terrain.fertility(c, r);
                }
            }
            if fert < 1.0 {
                return f64::NEG_INFINITY;
            }
            score += fert * 2.0;
        }
        // A jetty on a river beats one on a puddle: running water goes
        // somewhere, and a boat has to be able to leave.
        Some(Site::Shore) => {
            let mut close = f64::INFINITY;
            let mut river = false;
            for r in row - 4..=row + def.h + 4 {
                for c in col - 4..=col + def.w + 4 {
                    if !sim.terrain.navigable(c, r) {
                        continue;
                    }
                    let d = ((c - col) as f64).hypot((r - row) as f64);
                    if d < close {
                        close = d;
                    }
                    river |= sim.terrain.is_river(c, r);
                }
            }
            if !close.is_finite() {
                return f64::NEG_INFINITY;
            }
            score += 10.0 - close * 2.0 + if river { 6.0 } else { 0.0 };
        }
        None => {}
    }
    if def.category == Category::Gather {
        if let Some(Job::Harvest { classes, .. }) = &def.job {
            // Camps want standing growth of the classes they cut.
            let radius = if def.radius > 0.0 { def.radius } else { 12.0 };
            let mut mass = 0.0;
            sim.plant_index.near(col, row, radius, |mark| {
                if !classes.contains(&mark.class) {
                    return;
                }
                let d = ((mark.col - col) as f64).hypot((mark.row - row) as f64);
                if d < radius {
                    mass += mark.mass as f64 / (1.0 + d * 0.15);
                }
            });
            if mass < 2.0 {
                return f64::NEG_INFINITY;
            }
            score += clamp(mass * 0.25, 0.0, 14.0);
        }
    }
    let center = sim.colonies[ci].center;
    let dist = ((col - center.0) as f64).hypot((row - center.1) as f64);
    score -= dist * 0.35;
    // Two towns do not grow into each other: the further this site is from
    // somebody else's center, the better.
    for other in &sim.colonies {
        if other.id == colony {
            continue;
        }
        let d = ((col - other.center.0) as f64).hypot((row - other.center.1) as f64);
        score -= (24.0 - d).max(0.0) * 0.6;
    }
    // Homes and workshops like to be near a store; gathering wants to be out.
    if let Some(si) = sim.nearest_store(colony, col, row) {
        let store = &sim.buildings[si];
        let sd = ((store.col - col) as f64).hypot((store.row - row) as f64);
        score -= if def.category == Category::Gather {
            (sd - 14.0).max(0.0) * 0.4
        } else {
            sd * 0.3
        };
    }
    score
}

pub fn find_site_near(
    sim: &Settlement,
    state: &State,
    ci: usize,
    def: &BuildingDef,
    col: i32,
    row: i32,
    radius: i32,
) -> Option<(i32, i32)> {
    let mut best = None;
    let mut best_score = f64::NEG_INFINITY;
    for r in row - radius..=row + radius {
        for c in col - radius..=col + radius {
            if !can_place_at(sim, state, def, c, r) {
                continue;
            }
            let score = site_score(sim, ci, def, c, r);
            if score > best_score {
                best_score = score;
                best = Some((c, r));
            }
        }
    }
    if best_score > f64::NEG_INFINITY {
        best
    } else {
        None
    }
}

pub fn find_site(sim: &Settlement, state: &State, ci: usize, def: &BuildingDef) -> Option<(i32, i32)> {
    let center = sim.colonies[ci].center;
    let radius = state.civ.build.sprawl.max(4);
    find_site_near(sim, state, ci, def, center.0, center.1, radius)
}

pub fn plan(sim: &mut Settlement, state: &State, ci: usize) {
    let cfg = &state.civ.build;
    if !cfg.auto_build || ci >= sim.colonies.len() {
        return;
    }
    let colony = sim.colonies[ci].id;
    // Wall pieces have a budget of their own, or a town that decided to ring
    // itself would stop building anything else until the ring was closed.
    let sites = sim
        .buildings
        .iter()
        .filter(|b| !b.built && b.colony == colony && !b.def.structure.perimeter())
        .count() as i32;
    if sites >= cfg.max_sites {
        return;
    }
    let want = match plan_next(sim, state, ci) {
        Some(def) => def,
        None => return,
    };
    let site = match find_site(sim, state, ci, want) {
        Some(s) => s,
        None => return,
    };
    sim.place_building(state, ci, want.id, site.0, site.1, false);
}

/// A settlement only wants so many of one thing. Homes answer to the housing
/// need and storage to how full the store is, so neither is capped by head
/// count.
pub fn type_cap(def: &BuildingDef, state: &State, pop: usize) -> i32 {
    if def.housing > 0 || def.storage > 0.0 {
        return 99;
    }
    match state.civ.build.per_type.get(def.category) {
        Some(per) if per > 0 => 1 + (pop as i32 / per),
        _ => 99,
    }
}

/// Scores every unlocked building against what a colony is short of and returns
/// the best one it can plausibly pay for.
pub fn plan_next(sim: &Settlement, state: &State, ci: usize) -> Option<&'static BuildingDef> {
    let cfg = &state.civ.build;
    let colony = sim.colonies[ci].id;
    let pop = sim.colony_population(colony);
    let stock = &sim.colonies[ci].stock;
    let targets = stock_targets(&state.civ.economy, pop);
    let tally = tally_types(sim, colony);
    let counts = |id: &str| tally.get(id).copied().unwrap_or_default();
    let mut best: Option<&'static BuildingDef> = None;
    let mut best_score = 0.25;

    for def in BUILDINGS {
        if !def.planned {
            continue;
        }
        if !def.base && !sim.colonies[ci].unlocked.contains(def.id) {
            continue;
        }
        let have = counts(def.id);
        if have.all - have.built > 0 {
            continue;
        }
        if have.all >= type_cap(def, state, pop) {
            continue;
        }
        // No second workshop while the first one still has an empty bench.
        if def.slots > 0 && have.built > 0 && have.open > 0 {
            continue;
        }
        let mut score;

        if def.housing > 0 {
            let short = pop as i32 + cfg.housing_slack - sim.housing_capacity(colony);
            if short <= 0 {
                continue;
            }
            score = clamp(short as f64 / (def.housing as f64).max(1.0), 0.0, 3.0) * 1.6;
            // Prefer the best home the settlement can actually supply.
            score *= 0.6 + def.comfort * 0.8;
        } else if def.storage > 0.0 {
            let cap = sim.store_capacity(colony);
            let fill = if cap > 0.0 { stock_bulk(stock) / cap } else { 1.0 };
            if fill < 0.7 && cap > 0.0 {
                continue;
            }
            score = 1.4 + if cap == 0.0 { 2.0 } else { 0.0 };
        } else {
            match &def.job {
                Some(Job::Harvest { yields, .. }) => {
                    let res = yields[0].0;
                    let need = clamp(
                        (targets[res as usize] - stock[res as usize]) / targets[res as usize].max(1.0),
                        -1.0,
                        1.0,
                    );
                    let open = have.open as f64;
                    score = need * 1.5 - open * 0.35;
                    if res == Res::Food {
                        score += 0.4;
                    }
                }
                Some(Job::Mine { deposit, yields }) => {
                    let dep = sim.terrain.count_deposits(*deposit);
                    if dep.cells == 0 {
                        continue;
                    }
                    let res = yields[0].0;
                    let need = clamp(
                        (targets[res as usize] - stock[res as usize]) / targets[res as usize].max(1.0),
                        -1.0,
                        1.0,
                    );
                    score = need * 1.5 - have.open as f64 * 0.35;
                }
                Some(Job::Farm { .. }) => {
                    let need = clamp(
                        (targets[Res::Food as usize] - stock[Res::Food as usize])
                            / targets[Res::Food as usize].max(1.0),
                        -1.0,
                        1.0,
                    );
                    score = need * 1.7 - have.open as f64 * 0.3;
                }
                Some(Job::Craft { input, output, .. }) => {
                    let mut inputs: f64 = 1.0;
                    for &(res, n) in input.iter() {
                        inputs = inputs.min(stock[res as usize] / (n * 6.0).max(1.0));
                    }
                    let mut want = 0.0;
                    for &(res, _) in output.iter() {
                        want += clamp(
                            (targets[res as usize] - stock[res as usize])
                                / targets[res as usize].max(1.0),
                            -1.0,
                            1.0,
                        );
                    }
                    score = want * inputs * 1.4 - have.open as f64 * 0.3;
                }
                Some(Job::Research) => {
                    score = 1.1 - have.built as f64 * 0.6 - have.open as f64 * 0.3;
                }
                Some(Job::Trade) => {
                    score = 1.0 - have.built as f64 * 1.5;
                }
                // An inn earns its keep on the people sleeping in the open,
                // which is what a run of house rebuilds creates, and on the
                // size of the town: somewhere for a stranger to stay is worth
                // having before anyone actually needs it.
                Some(Job::Innkeep) => {
                    let roofless = sim.roofless(colony) as f64;
                    score = 0.6 + roofless * 0.5 + (pop as f64 / 24.0) - have.built as f64 * 1.2;
                }
                // A dock is only worth anything once there is somewhere to sail
                // to.
                Some(Job::Ferry) => {
                    let partners = sim.colonies.len() as f64 - 1.0;
                    score = partners * 0.8 - have.built as f64 * 2.0;
                }
                // A lamp is worth having once there is a town to light, and one
                // is worth more than the second: the first turns a black street
                // into a lit one.
                None if def.light > 0.0 => {
                    score = pop as f64 / 8.0 - have.built as f64 * 0.7;
                }
                None if def.health > 0.0 => {
                    score = 0.9 - have.built as f64 * 0.5;
                }
                _ => score = 0.0,
            }
        }

        score *= state.civ.build.weights.get(def.category);
        if !can_supply(sim, state, ci, def) {
            continue;
        }
        if score > best_score {
            best_score = score;
            best = Some(def);
        }
    }
    best
}

/// How many of each building type a colony has, in one pass.
///
/// The planner asks "how many of these are there, how many are finished, how
/// many benches are empty" for every type in the catalog, and each of those
/// questions used to walk the whole building list: twenty-five types times
/// several questions times every building in the world, eight times a simulated
/// second. One pass answers all of them.
#[derive(Clone, Copy, Default)]
struct TypeCount {
    all: i32,
    built: i32,
    open: i32,
}

fn tally_types(sim: &Settlement, colony: i32) -> HashMap<&'static str, TypeCount> {
    let mut out: HashMap<&'static str, TypeCount> = HashMap::new();
    for b in &sim.buildings {
        if b.colony != colony {
            continue;
        }
        let e = out.entry(b.def.id).or_default();
        e.all += 1;
        if b.built {
            e.built += 1;
            e.open += b.def.slots as i32 - b.workers.len() as i32;
        }
    }
    out
}

/// Whether a colony could plausibly pay for this: every material either in
/// store already, or already part way there with something standing that makes
/// more of it.
///
/// Without this a town queues a site it has no way of finishing, and then
/// never queues anything else because the site is still open.
pub fn can_supply(sim: &Settlement, state: &State, ci: usize, def: &BuildingDef) -> bool {
    let colony = sim.colonies[ci].id;
    for &(res, n) in &scaled_cost(def, &state.civ.build) {
        let have = sim.colonies[ci].stock[res as usize];
        if have >= n {
            continue;
        }
        if have >= n * 0.15 && producer_of(sim, colony, res).is_some() {
            continue;
        }
        return false;
    }
    true
}

pub fn producer_of(sim: &Settlement, colony: i32, res: Res) -> Option<usize> {
    for (i, b) in sim.buildings.iter().enumerate() {
        if !b.built || b.colony != colony {
            continue;
        }
        if let Some(job) = &b.def.job {
            if job.produces().iter().any(|&(r, _)| r == res) {
                return Some(i);
            }
        }
    }
    None
}

// ---- the ring ------------------------------------------------------------

/// The rectangle a town would ring itself with: everything it has raised, plus
/// a margin, clamped to the map.
///
/// Measured against the town rather than against the wall, because a ring that
/// counted its own pieces would push itself one cell further out on every pass
/// and never close.
pub fn wall_ring(sim: &Settlement, ci: usize, margin: i32) -> Option<(i32, i32, i32, i32)> {
    let colony = sim.colonies[ci].id;
    let center = sim.colonies[ci].center;
    let (mut x0, mut y0) = center;
    let (mut x1, mut y1) = center;
    let mut any = false;
    for b in &sim.buildings {
        // Not the wall itself, or the ring would push itself one cell further
        // out on every pass; and not the outlying camps, or a woodcutter
        // sixteen cells into the forest would drag the whole ring after it.
        if b.colony != colony
            || b.def.structure.perimeter()
            || b.def.category == Category::Gather
        {
            continue;
        }
        x0 = x0.min(b.col);
        y0 = y0.min(b.row);
        x1 = x1.max(b.col + b.w - 1);
        y1 = y1.max(b.row + b.h - 1);
        any = true;
    }
    if !any {
        return None;
    }
    let m = margin.max(1);
    let x0 = (x0 - m).max(0);
    let y0 = (y0 - m).max(0);
    let x1 = (x1 + m).min(sim.world().cols - 1);
    let y1 = (y1 + m).min(sim.world().rows - 1);
    if x1 - x0 < 4 || y1 - y0 < 4 {
        return None;
    }
    Some((x0, y0, x1, y1))
}

pub fn on_ring(ring: (i32, i32, i32, i32), col: i32, row: i32) -> bool {
    let (x0, y0, x1, y1) = ring;
    col >= x0 && col <= x1 && row >= y0 && row <= y1 && (col == x0 || col == x1 || row == y0 || row == y1)
}

/// Every cell of the ring, each with the direction that points away from the
/// town. The outward direction is what the safety check needs: it is the side
/// somebody would be walking home from.
fn ring_cells(ring: (i32, i32, i32, i32)) -> Vec<(i32, i32, (i32, i32))> {
    let (x0, y0, x1, y1) = ring;
    let mut out = Vec::new();
    for c in x0..=x1 {
        for (r, dy) in [(y0, -1), (y1, 1)] {
            let dx = if c == x0 {
                -1
            } else if c == x1 {
                1
            } else {
                0
            };
            out.push((c, r, (dx, dy)));
        }
    }
    for r in y0 + 1..y1 {
        out.push((x0, r, (-1, 0)));
        out.push((x1, r, (1, 0)));
    }
    out
}

/// Whether an ordinary building stands against this cell. A ring keeps clear
/// of the doors it is protecting, or a wall raised flush against a house takes
/// away the only side anybody could walk up to it from.
fn touches_building(sim: &Settlement, col: i32, row: i32) -> bool {
    for r in row - 1..=row + 1 {
        for c in col - 1..=col + 1 {
            if !sim.in_bounds(c, r) {
                continue;
            }
            let id = sim.build_grid[sim.idx(c, r)];
            if id == 0 {
                continue;
            }
            if let Some(bi) = sim.building_index(id) {
                if !sim.buildings[bi].def.structure.perimeter() {
                    return true;
                }
            }
        }
    }
    false
}

/// Where the next piece of a town's ring goes.
///
/// Gates are drawn to the busiest cells of the ring and wall to the quietest,
/// which is the whole siting rule: the ways through end up on the roads people
/// have already worn, and the blank stretches go over ground nobody crosses.
///
/// Nothing is sited that would shut the town in. Before a piece is accepted,
/// the ground just outside it has to still have a way to the middle of the
/// town with that piece closed, so the ring tightens around its gates and
/// stops at the last gap if there are none.
pub fn ring_site(
    sim: &mut Settlement,
    state: &State,
    ci: usize,
    def: &BuildingDef,
) -> Option<(i32, i32)> {
    let ring = wall_ring(sim, ci, state.civ.build.wall_margin)?;
    let center = sim.colonies[ci].center;
    let gate = def.structure == Structure::Gate;
    // Ways through are spread around the ring rather than clustered. Without
    // this they all land on the same worn cell and its neighbors, and a town
    // ends up with one gap four cells wide instead of gates on three sides.
    const GATE_APART: i32 = 6;
    let colony = sim.colonies[ci].id;
    let others: Vec<(i32, i32)> = sim
        .buildings
        .iter()
        .filter(|b| b.colony == colony && b.def.structure == Structure::Gate)
        .map(|b| (b.col, b.row))
        .collect();
    let mut cands: Vec<(f64, i32, i32, (i32, i32))> = Vec::new();
    for (c, r, out) in ring_cells(ring) {
        if !can_place_at(sim, state, def, c, r) || touches_building(sim, c, r) {
            continue;
        }
        if gate
            && others
                .iter()
                .any(|&(gc, gr)| (gc - c).abs().max((gr - r).abs()) < GATE_APART)
        {
            continue;
        }
        let worn = sim.traffic[sim.idx(c, r)] as f64;
        cands.push((if gate { worn } else { -worn }, c, r, out));
    }
    cands.sort_by(|a, z| {
        z.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&z.1))
            .then(a.2.cmp(&z.2))
    });
    for (_, c, r, out) in cands.into_iter().take(24) {
        let outside = (c + out.0, r + out.1);
        // Ground that is already water or already walled strands nobody.
        if !sim.walkable(outside.0, outside.1) {
            return Some((c, r));
        }
        if sim.path_exists_without((c, r), outside, center) {
            return Some((c, r));
        }
    }
    None
}

/// The pieces a town would raise next, best first: a gate while the ring has
/// fewer than it wants, then the best wall it knows how to build.
fn wall_pieces(sim: &Settlement, state: &State, ci: usize) -> Vec<&'static BuildingDef> {
    let cfg = &state.civ.build;
    let known = |id: &str| {
        building_by_id(id).filter(|d| d.base || sim.colonies[ci].unlocked.contains(d.id))
    };
    let wall = known("rampart").or_else(|| known("palisade"));
    let mut out = Vec::new();
    if let Some(ring) = wall_ring(sim, ci, cfg.wall_margin) {
        let colony = sim.colonies[ci].id;
        let gates = sim
            .buildings
            .iter()
            .filter(|b| {
                b.colony == colony
                    && b.def.structure == Structure::Gate
                    && on_ring(ring, b.col, b.row)
            })
            .count() as i32;
        if gates < cfg.wall_gates.max(1) {
            out.extend(known("gate"));
        }
    }
    out.extend(wall);
    out
}

/// One piece of wall per pass, on its own budget.
pub fn plan_walls(sim: &mut Settlement, state: &State, ci: usize) {
    let cfg = &state.civ.build;
    if !cfg.walls || !cfg.auto_build || ci >= sim.colonies.len() {
        return;
    }
    let colony = sim.colonies[ci].id;
    // A ring is the work of a town, not of a village. Below this a settlement
    // has better uses for every plank it owns.
    if (sim.colony_population(colony) as i32) < cfg.wall_population {
        return;
    }
    let going_up = sim
        .buildings
        .iter()
        .filter(|b| !b.built && b.colony == colony && b.def.structure.perimeter())
        .count() as i32;
    if going_up >= cfg.wall_sites.max(0) {
        return;
    }
    for def in wall_pieces(sim, state, ci) {
        if !can_supply(sim, state, ci, def) {
            continue;
        }
        if let Some(site) = ring_site(sim, state, ci, def) {
            sim.place_building(state, ci, def.id, site.0, site.1, false);
            return;
        }
    }
}
