//! Seeded RNG (mulberry32). Every stochastic part of the sim draws from an
//! explicit stream so a run can be reproduced from a single seed.

#[derive(Clone, Debug)]
pub struct Rng {
    state: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    /// The next draw in [0,1). Not an iterator: the stream never ends.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x6d2b79f5);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64) / 4294967296.0
    }

    pub fn range(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min) * self.next()
    }

    pub fn int(&mut self, min: i32, max: i32) -> i32 {
        min + (self.next() * (max - min + 1) as f64).floor() as i32
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.next() < p
    }

    pub fn sign(&mut self) -> f64 {
        if self.next() < 0.5 {
            -1.0
        } else {
            1.0
        }
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let i = (self.next() * items.len() as f64).floor() as usize % items.len();
        &items[i]
    }

    pub fn seed(&mut self) -> u32 {
        (self.next() * 4294967296.0) as u32
    }
}
