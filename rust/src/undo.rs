//! Undo for the whole project.
//!
//! A step is a copy of the project as it stood before an edit, taken before the
//! edit happens. Snapshots rather than inverse operations, because most of what
//! the tool does has no inverse worth writing: a flood fill, a resize that
//! crops, a merge that folds two layers into one, a parameter that resets a
//! simulation. A project is a few tens of kilobytes and every edit is something
//! a person did by hand, so copying it is cheaper than the bookkeeping the
//! alternative needs.
//!
//! Redo is the same mechanism read the other way: undoing snapshots the current
//! state on the way past, so the two stacks hold the same kind of thing.

use crate::state::State;

/// How far back the tool remembers, and how much of the pixel budget that is
/// allowed to cost. A project holding sheets near their caps is megabytes on
/// its own, so the depth alone is not a bound worth trusting; the oldest steps
/// are dropped when either runs out.
const MAX_STEPS: usize = 80;
const MAX_PIXELS: usize = 4 << 20;

/// A control that is still being worked adds to the step it already made rather
/// than making another. Dragging a slider fires an event a frame; without this
/// one drag would fill the history and undoing it would take a hundred presses.
const COALESCE_MS: f64 = 900.0;

pub struct Step {
    state: Box<State>,
    /// What was being changed. Two edits from the same control in quick
    /// succession are one step; two from different controls never are.
    key: String,
    at: f64,
}

impl Step {
    /// Roughly what the step is holding, in pixels, for the budget. The two
    /// pixel buffers are all that varies by more than a rounding error.
    fn pixels(&self) -> usize {
        let materials = self.state.materials.atlas.px.len()
            + self.state.materials.samplers.iter().map(|s| s.px.len()).sum::<usize>();
        let art: usize = self
            .state
            .art
            .sheets
            .iter()
            .map(|s| {
                s.layers
                    .iter()
                    .map(|l| l.cels.iter().map(|c| c.px.len()).sum::<usize>())
                    .sum::<usize>()
            })
            .sum();
        materials + art
    }
}

#[derive(Default)]
pub struct History {
    done: Vec<Step>,
    undone: Vec<Step>,
}

impl History {
    /// Records the project as it stands, before something changes it. `key`
    /// names the control doing the changing; `coalesce` is for the controls a
    /// person holds rather than presses.
    pub fn record(&mut self, state: &State, key: &str, coalesce: bool, now: f64) {
        if coalesce {
            if let Some(last) = self.done.last_mut() {
                if last.key == key && now - last.at < COALESCE_MS {
                    last.at = now;
                    return;
                }
            }
        }
        // Anything that had been undone is dropped: a new edit is a new branch,
        // and keeping the old one would mean a redo that puts back something
        // that was never there.
        self.undone.clear();
        self.done.push(Step { state: Box::new(state.clone()), key: key.to_string(), at: now });
        trim(&mut self.done);
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
    }

    pub fn undo(&mut self, state: &mut State) -> bool {
        step(&mut self.done, &mut self.undone, state)
    }

    pub fn redo(&mut self, state: &mut State) -> bool {
        step(&mut self.undone, &mut self.done, state)
    }
}

/// Moves one step from one stack to the other, leaving behind what it replaced.
fn step(from: &mut Vec<Step>, to: &mut Vec<Step>, state: &mut State) -> bool {
    let step = match from.pop() {
        Some(s) => s,
        None => return false,
    };
    to.push(Step { state: Box::new(state.clone()), key: step.key.clone(), at: step.at });
    trim(to);
    *state = *step.state;
    true
}

fn trim(stack: &mut Vec<Step>) {
    while stack.len() > MAX_STEPS {
        stack.remove(0);
    }
    let mut total: usize = stack.iter().map(|s| s.pixels()).sum();
    while total > MAX_PIXELS && stack.len() > 1 {
        total -= stack[0].pixels();
        stack.remove(0);
    }
}
