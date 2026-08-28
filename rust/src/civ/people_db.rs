//! The register of settlers.
//!
//! Every person who has ever lived in the world keeps a slot here for as long
//! as anyone might ask about them, which is what makes parentage, marriages
//! and obituaries answerable questions rather than strings copied out before
//! the record was dropped.
//!
//! Slots are stable. The dead are marked, not removed, so an index handed to a
//! task this tick still means the same person next tick, and `id` lookups are
//! a hash rather than a scan of the whole population. The only thing that ever
//! moves a slot is `prune`, which drops the oldest dead once the archive grows
//! past its cap and is called at a point where nothing is holding an index.

use std::collections::HashMap;
use std::ops::{Index, IndexMut};

use serde::{Deserialize, Serialize};

use crate::civ::people::Person;

#[derive(Serialize, Deserialize)]
pub struct PeopleDb {
    all: Vec<Person>,
    /// Both of these are worked out from `all`, so a register read back off a
    /// save rebuilds them rather than carrying them.
    #[serde(skip)]
    by_id: HashMap<u32, usize>,
    /// Indices of the living, ascending, so iteration order is a function of
    /// birth order and nothing else.
    #[serde(skip)]
    live: Vec<usize>,
    next_id: u32,
    buried: u32,
}

impl Default for PeopleDb {
    fn default() -> Self {
        PeopleDb::new()
    }
}

impl PeopleDb {
    pub fn new() -> Self {
        PeopleDb {
            all: Vec::new(),
            by_id: HashMap::new(),
            live: Vec::new(),
            next_id: 1,
            buried: 0,
        }
    }

    pub fn clear(&mut self) {
        self.all.clear();
        self.by_id.clear();
        self.live.clear();
        self.next_id = 1;
        self.buried = 0;
    }

    /// The id the next settler will be given. Ids are never reused, so a stale
    /// reference resolves to nothing rather than to a stranger.
    pub fn claim_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Files a new settler and returns their slot.
    pub fn insert(&mut self, person: Person) -> usize {
        let index = self.all.len();
        self.by_id.insert(person.id, index);
        if person.alive {
            self.live.push(index);
        }
        self.all.push(person);
        index
    }

    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.by_id.get(&id).copied()
    }

    pub fn get(&self, id: u32) -> Option<&Person> {
        self.index_of(id).map(|i| &self.all[i])
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Person> {
        match self.index_of(id) {
            Some(i) => Some(&mut self.all[i]),
            None => None,
        }
    }

    pub fn is_alive(&self, id: u32) -> bool {
        self.get(id).is_some_and(|p| p.alive)
    }

    /// Drops a slot out of the living list. The record stays where it is.
    ///
    /// This is the authority on whether somebody has been buried, not the
    /// `alive` flag: a task sets that flag the moment it decides a settler has
    /// died, and the burial happens afterwards. Returning false is what stops
    /// the same death being counted, logged and inherited from on every tick
    /// for the rest of the run.
    pub fn retire(&mut self, index: usize) -> bool {
        let before = self.live.len();
        self.live.retain(|&i| i != index);
        if self.live.len() == before {
            return false;
        }
        self.all[index].alive = false;
        self.buried += 1;
        true
    }

    pub fn buried(&self) -> u32 {
        self.buried
    }

    /// Living settlers.
    pub fn count(&self) -> usize {
        self.live.len()
    }

    /// Every slot, living and dead.
    pub fn slots(&self) -> usize {
        self.all.len()
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// Snapshot of the living slots, for a loop that may bury somebody.
    pub fn live_indices(&self) -> Vec<usize> {
        self.live.clone()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Person> + '_ {
        self.live.iter().map(move |&i| &self.all[i])
    }

    pub fn iter_indexed(&self) -> impl Iterator<Item = (usize, &Person)> + '_ {
        self.live.iter().map(move |&i| (i, &self.all[i]))
    }

    /// Runs a change over every living settler. Written as a callback rather
    /// than a mutable iterator so the live list stays borrowed by nothing.
    pub fn for_each_live(&mut self, mut f: impl FnMut(&mut Person)) {
        for k in 0..self.live.len() {
            let i = self.live[k];
            f(&mut self.all[i]);
        }
    }

    /// Everyone on file, in the order they were born.
    pub fn archive(&self) -> &[Person] {
        &self.all
    }

    pub fn archive_mut(&mut self) -> &mut [Person] {
        &mut self.all
    }

    /// Drops the oldest dead once the archive outgrows its cap, then rebuilds
    /// the index. Call this only where no index is being held across it.
    pub fn prune(&mut self, keep_dead: usize) {
        let dead = self.all.iter().filter(|p| !p.alive).count();
        if dead <= keep_dead {
            return;
        }
        let mut drop_left = dead - keep_dead;
        let mut keep = Vec::with_capacity(self.all.len() - drop_left);
        for person in self.all.drain(..) {
            if !person.alive && drop_left > 0 {
                drop_left -= 1;
                continue;
            }
            keep.push(person);
        }
        self.all = keep;
        self.reindex();
    }

    /// Public because a register read back off a save arrives without either
    /// of the two lookups, and this is what fills them in.
    pub fn reindex(&mut self) {
        self.by_id.clear();
        self.live.clear();
        for (i, p) in self.all.iter().enumerate() {
            self.by_id.insert(p.id, i);
            if p.alive {
                self.live.push(i);
            }
        }
    }
}

impl Index<usize> for PeopleDb {
    type Output = Person;

    fn index(&self, i: usize) -> &Person {
        &self.all[i]
    }
}

impl IndexMut<usize> for PeopleDb {
    fn index_mut(&mut self, i: usize) -> &mut Person {
        &mut self.all[i]
    }
}

impl<'a> IntoIterator for &'a PeopleDb {
    type Item = &'a Person;
    type IntoIter = Box<dyn Iterator<Item = &'a Person> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}
