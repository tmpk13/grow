//! What people make of each other.
//!
//! Everyone a person has stood next to for long enough keeps a slot in that
//! person's memory: how they met, how often since, and what the two of them
//! have come to think of one another. Nothing about the bookkeeping is
//! symmetric except by construction - both sides get their own record, and
//! both are written at the same moment, so a one sided regard is expressible
//! even though nothing currently creates one.
//!
//! The pass that finds who is near whom is the only part of this that could
//! get expensive, so it runs on its own timer over a coarse bucket grid rather
//! than over every pair of people, and each person registers a bounded number
//! of encounters per pass. A market square with forty people in it therefore
//! costs forty times a small constant, not sixteen hundred.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::civ::people::Traits;
use crate::civ::settlement::Settlement;
use crate::state::State;
use crate::util::{clamp, clamp01, hash2};

/// What a bond reads as, once the number behind it is turned into a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tie {
    Spouse,
    Kin,
    Friend,
    Rival,
    Known,
}

impl Tie {
    pub fn label(self) -> &'static str {
        match self {
            Tie::Spouse => "married",
            Tie::Kin => "kin",
            Tie::Friend => "friend",
            Tie::Rival => "rival",
            Tie::Known => "known",
        }
    }
}

/// One person's standing record of another.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bond {
    pub who: u32,
    /// From -1, a feud, through 0, a face in the street, to 1, devotion.
    pub affinity: f32,
    /// The day they first crossed paths.
    pub met: i32,
    /// Times they have crossed paths since, which is what separates a
    /// neighbor from somebody seen once at a landing.
    pub meetings: u32,
    /// Family. A kin bond is a fact rather than a feeling, and is never
    /// forgotten to make room for a stranger.
    pub kin: bool,
}

impl Bond {
    pub fn new(who: u32, kin: bool, day: i32) -> Bond {
        Bond { who, affinity: 0.0, met: day, meetings: 0, kin }
    }

    pub fn tie(&self, spouse: u32, friend_at: f64) -> Tie {
        if self.who == spouse && spouse != 0 {
            return Tie::Spouse;
        }
        if self.affinity as f64 >= friend_at {
            return Tie::Friend;
        }
        if (self.affinity as f64) <= -friend_at {
            return Tie::Rival;
        }
        if self.kin {
            return Tie::Kin;
        }
        Tie::Known
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SocialConfig {
    pub enabled: bool,
    /// Simulated seconds between passes over who is standing near whom.
    pub interval: f64,
    /// How close two people have to be to notice each other, in cells.
    pub radius: f64,
    /// Bonds one person carries. The faintest are forgotten first, and kin
    /// are never forgotten at all.
    pub memory: usize,
    /// How far one meeting moves a bond toward what the two make of each other.
    pub warmth: f64,
    /// Affinity at which a bond reads as a friendship, and its negative at
    /// which it reads as a feud.
    pub friend_at: f64,
    /// What good company is worth to somebody's contentment.
    pub company: f64,
    /// How much affinity decides between two otherwise comparable matches.
    /// Its say falls away as the two ages diverge, so this chooses between
    /// people of a generation rather than across generations.
    pub courtship: f64,
    /// Encounters one person may register in a single pass. This is what
    /// bounds the cost of a crowd.
    pub max_meetings: usize,
}

impl Default for SocialConfig {
    fn default() -> Self {
        SocialConfig {
            enabled: true,
            interval: 2.0,
            radius: 3.0,
            memory: 24,
            warmth: 0.06,
            friend_at: 0.45,
            company: 0.18,
            courtship: 2.5,
            max_meetings: 6,
        }
    }
}

/// What two people are likely to make of each other, before any of it has
/// happened.
///
/// Shared temperament and nothing else, plus a stable per pair draw so some
/// people simply never take to each other however alike they look on paper.
/// How outgoing the two are is deliberately absent: it decides how *fast* a
/// bond forms, below, and letting it also decide the destination turns
/// sociability into a single hidden score for how likeable somebody is. That
/// is worth spelling out, because it is not a rounding detail. Marriage is
/// already gated on sociability, so a town whose affinities also favor the
/// outgoing marries its sociable half to itself and leaves the rest single.
fn regard(a: &Traits, b: &Traits, kin: bool, jitter: f64) -> f64 {
    let alike = 1.0
        - (a.sociability - b.sociability).abs() * 0.4
        - (a.diligence - b.diligence).abs() * 0.3
        - (a.curiosity - b.curiosity).abs() * 0.3;
    let family = if kin { 0.3 } else { 0.0 };
    // Centered on how alike two people happen to be rather than on zero: two
    // temperaments that are a poor match end up genuinely negative, which is
    // what makes a friendship worth noticing.
    clamp((alike - 0.72) * 2.4 + family + jitter, -1.0, 1.0)
}

/// A number that belongs to the pair rather than to either of them, so the two
/// sides of a bond drift toward the same place and it stays the same on every
/// run of the same seed.
fn pair_jitter(a: u32, b: u32) -> f64 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    (hash2(lo as i32, hi as i32, 5501) - 0.5) * 0.9
}

/// One encounter: both sides remember it, and both move a little toward what
/// they make of each other.
fn meet(sim: &mut Settlement, cfg: &SocialConfig, ai: usize, bi: usize) {
    let day = sim.day;
    let (aid, bid) = (sim.people[ai].id, sim.people[bi].id);
    if aid == bid {
        return;
    }
    let kin = sim.kin(ai, bi);
    let jitter = pair_jitter(aid, bid);
    let target = {
        let (a, b) = (&sim.people[ai], &sim.people[bi]);
        regard(&a.traits, &b.traits, kin, jitter)
    };
    let rate = clamp01(
        cfg.warmth * (0.5 + (sim.people[ai].traits.sociability + sim.people[bi].traits.sociability) * 0.5),
    );
    for (self_i, other_id) in [(ai, bid), (bi, aid)] {
        let before = sim.people[self_i]
            .bond_with(other_id)
            .map(|bond| bond.affinity as f64)
            .unwrap_or(0.0);
        let slot = match sim.people[self_i].remember(other_id, kin, day, cfg.memory) {
            Some(slot) => slot,
            None => continue,
        };
        let after = {
            let bond = &mut sim.people[self_i].bonds[slot];
            bond.meetings += 1;
            bond.kin |= kin;
            bond.affinity += ((target - bond.affinity as f64) * rate) as f32;
            bond.affinity as f64
        };
        // Said once, on the crossing, rather than every pass afterwards.
        if before < cfg.friend_at && after >= cfg.friend_at {
            let name = sim.people.get(other_id).map(|q| q.name.clone());
            if let Some(name) = name {
                sim.people[self_i].log(day, format!("took to {name}"));
            }
        } else if before > -cfg.friend_at && after <= -cfg.friend_at {
            let name = sim.people.get(other_id).map(|q| q.name.clone());
            if let Some(name) = name {
                sim.people[self_i].log(day, format!("fell out with {name}"));
            }
        }
    }
}

/// Finds who is standing near whom and lets them notice each other.
///
/// Only people who are actually out on the ground take part: somebody asleep
/// behind their own door or at sea on a boat is not in the street to be met.
pub fn social_tick(sim: &mut Settlement, state: &State, dt: f64) {
    let cfg = state.civ.social;
    if !cfg.enabled {
        return;
    }
    sim.social_timer -= dt;
    if sim.social_timer > 0.0 {
        return;
    }
    sim.social_timer = cfg.interval.max(0.1);

    let radius = cfg.radius.max(0.5);
    let bucket = radius.ceil().max(1.0) as i32;
    let mut buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (pi, p) in sim.people.iter_indexed() {
        if p.indoors() || p.aboard != 0 {
            continue;
        }
        let key = ((p.x as i32) / bucket, (p.y as i32) / bucket);
        buckets.entry(key).or_default().push(pi);
    }

    // Each pair is offered once, from the lower slot, so a meeting is not
    // registered twice and the walk stays over the near neighbors only.
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut budget: HashMap<usize, usize> = HashMap::new();
    let r2 = radius * radius;
    for (&(bx, by), here) in &buckets {
        for &ai in here {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let there = match buckets.get(&(bx + dx, by + dy)) {
                        Some(v) => v,
                        None => continue,
                    };
                    for &bi in there {
                        if bi <= ai {
                            continue;
                        }
                        let (a, b) = (&sim.people[ai], &sim.people[bi]);
                        if (a.x - b.x).powi(2) + (a.y - b.y).powi(2) > r2 {
                            continue;
                        }
                        let na = budget.entry(ai).or_insert(0);
                        if *na >= cfg.max_meetings {
                            continue;
                        }
                        *na += 1;
                        let nb = budget.entry(bi).or_insert(0);
                        if *nb >= cfg.max_meetings {
                            continue;
                        }
                        *nb += 1;
                        pairs.push((ai, bi));
                    }
                }
            }
        }
    }
    // Sorted, so the order encounters are applied in is a function of the map
    // rather than of how a hash map happened to lay itself out.
    pairs.sort_unstable();
    for (ai, bi) in pairs {
        meet(sim, &cfg, ai, bi);
    }

    let friend_at = cfg.friend_at as f32;
    let company = cfg.company;
    sim.people.for_each_live(|p| {
        let mut friends = 0;
        let mut rivals = 0;
        for bond in &p.bonds {
            if bond.affinity >= friend_at {
                friends += 1;
            } else if bond.affinity <= -friend_at {
                rivals += 1;
            }
        }
        p.friends = friends;
        p.rivals = rivals;
        // One number for the whole social ledger, because the needs tick has
        // no business reading a list of bonds every frame.
        p.regard = clamp01(friends as f64 / 3.0) * company
            - clamp01(rivals as f64 / 3.0) * company * 0.6;
    });
}
