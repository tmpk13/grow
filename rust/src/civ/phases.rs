//! Where a settlement tick spends its time, measured from inside.
//!
//! `perf` needs kernel permissions this machine does not grant, so the phases
//! of `Settlement::step` are timed directly: each phase holds a guard while it
//! runs, and the guard adds its elapsed time to a per-thread tally when it is
//! dropped. Set `GROW_PHASES` in the environment and the headless binaries
//! print the tally when they finish; without it the guards do nothing beyond
//! one branch, and on wasm they compile to nothing at all, `Instant` having no
//! clock to read there.
//!
//! The tally is per thread rather than shared because the settlement itself is
//! single threaded and the tests run one settlement per thread.

#[derive(Clone, Copy)]
pub enum Phase {
    Refresh,
    Plants,
    PlantIndex,
    Plan,
    People,
    Farms,
    Social,
    Production,
    Economy,
    Boats,
    Day,
    Traffic,
    Raster,
}

pub const PHASE_COUNT: usize = 13;

pub const PHASE_NAMES: [&str; PHASE_COUNT] = [
    "refresh", "plants", "index", "plan", "people", "farms", "social", "production", "economy",
    "boats", "day", "traffic", "raster",
];

pub use imp::{report, time, Guard};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::cell::RefCell;
    use std::sync::OnceLock;
    use std::time::Instant;

    use super::{Phase, PHASE_COUNT, PHASE_NAMES};

    thread_local! {
        static TALLY: RefCell<[f64; PHASE_COUNT]> = const { RefCell::new([0.0; PHASE_COUNT]) };
    }

    fn enabled() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("GROW_PHASES").is_some())
    }

    /// Times one phase for as long as the returned guard lives.
    pub fn time(phase: Phase) -> Guard {
        Guard { phase, start: enabled().then(Instant::now) }
    }

    pub struct Guard {
        phase: Phase,
        start: Option<Instant>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(start) = self.start {
                let dt = start.elapsed().as_secs_f64();
                TALLY.with(|t| t.borrow_mut()[self.phase as usize] += dt);
            }
        }
    }

    /// The tally so far, largest first, or None when timing is off.
    pub fn report() -> Option<String> {
        if !enabled() {
            return None;
        }
        TALLY.with(|t| {
            let tally = t.borrow();
            let total: f64 = tally.iter().sum();
            if total <= 0.0 {
                return Some("phases: nothing timed".to_string());
            }
            let mut rows: Vec<(usize, f64)> =
                tally.iter().cloned().enumerate().filter(|(_, s)| *s > 0.0).collect();
            rows.sort_by(|a, b| b.1.total_cmp(&a.1));
            let cols: Vec<String> = rows
                .iter()
                .map(|&(i, s)| {
                    format!("{} {:.2}s {:.1}%", PHASE_NAMES[i], s, s / total * 100.0)
                })
                .collect();
            Some(format!("phases, {total:.2}s in step: {}", cols.join("   ")))
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::Phase;

    pub struct Guard;

    pub fn time(_phase: Phase) -> Guard {
        Guard
    }

    pub fn report() -> Option<String> {
        None
    }
}
