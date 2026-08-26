//! Settler animations built from dropped images.
//!
//! A clip is one sheet of pixels read as a row of equal frames, plus how it is
//! played and how large it is drawn. The sheet is kept whole rather than cut
//! up, so changing the frame count re-reads the same image instead of asking
//! for it to be dropped again.
//!
//! There is one clip per motion, and a motion with nothing dropped on it falls
//! back to a related one, so a single walk sheet is enough to stand in for the
//! generated settler everywhere.

use serde::{Deserialize, Serialize};

use crate::civ::people::Person;

/// Frames a clip may hold, and the largest one frame may be. Both are here to
/// keep a project that lives in local storage from being filled by a
/// screenshot dropped on the panel by mistake; a sheet past either is scaled
/// down or cut short on the way in.
pub const MAX_FRAMES: i32 = 24;
pub const MAX_FRAME_PX: i32 = 64;

/// Alpha below this reads as nothing there. Sprites are blitted as opaque
/// pixels over the map, so an edge is either drawn or it is not.
pub const ALPHA_CUT: u8 = 128;

/// What a settler is doing, as far as the drawing is concerned. Everything the
/// simulation knows about a person folds down to one of these, and each one
/// can be given its own images and its own frame count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    Idle,
    Walk,
    Carry,
    Work,
    Sleep,
}

pub const MOTIONS: [Motion; 5] = [
    Motion::Idle,
    Motion::Walk,
    Motion::Carry,
    Motion::Work,
    Motion::Sleep,
];

pub const MOTION_COUNT: usize = MOTIONS.len();

impl Motion {
    pub fn label(self) -> &'static str {
        match self {
            Motion::Idle => "Standing",
            Motion::Walk => "Walking",
            Motion::Carry => "Carrying",
            Motion::Work => "Working",
            Motion::Sleep => "Sleeping",
        }
    }

    /// The short name the clip travels under in a project file.
    pub fn key(self) -> &'static str {
        match self {
            Motion::Idle => "idle",
            Motion::Walk => "walk",
            Motion::Carry => "carry",
            Motion::Work => "work",
            Motion::Sleep => "sleep",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Motion::Idle => "on their feet with nowhere to be",
            Motion::Walk => "on a path, hands empty",
            Motion::Carry => "on a path with a load",
            Motion::Work => "stood at the work rather than walking to it",
            Motion::Sleep => "asleep out in the open",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Frames per unit and whether the clip is tied to the ground rather than
    /// the clock, for art that has just been dropped and has nothing tuned yet.
    /// A walk that is not tied to the ground either slides or runs on the spot,
    /// and a sleep that is tied to it never moves at all.
    pub fn playback(self) -> (f64, bool) {
        match self {
            Motion::Idle => (3.0, false),
            Motion::Walk => (6.0, true),
            Motion::Carry => (6.0, true),
            Motion::Work => (6.0, false),
            Motion::Sleep => (1.5, false),
        }
    }

    /// What this motion will settle for when nothing has been dropped on it.
    /// The first entry is always the motion itself, so one lookup answers both
    /// questions.
    pub fn chain(self) -> &'static [Motion] {
        match self {
            Motion::Idle => &[Motion::Idle, Motion::Walk],
            Motion::Walk => &[Motion::Walk, Motion::Idle],
            Motion::Carry => &[Motion::Carry, Motion::Walk, Motion::Idle],
            Motion::Work => &[Motion::Work, Motion::Idle, Motion::Walk],
            Motion::Sleep => &[Motion::Sleep, Motion::Idle],
        }
    }
}

/// One image on its way into a sheet: width, height and packed pixels.
pub type Frame = (i32, i32, Vec<u32>);

/// One animation: a sheet of frames laid left to right, and how it is played.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Clip {
    /// The sheet, at whatever size it arrived, scaled only if a frame was over
    /// the cap. Frames are read out of it by dividing, which is what lets the
    /// count be changed after the drop.
    pub w: i32,
    pub h: i32,
    #[serde(with = "crate::sampler::px_hex")]
    pub px: Vec<u32>,
    pub frames: i32,
    /// Frames per second, or frames per cell walked when `stride` is set.
    pub fps: f64,
    /// Tie the frame to ground covered rather than to the clock, so a walk
    /// never slides and never runs on the spot.
    pub stride: bool,
    /// Drawn height, in map cells. Width follows from the frame's shape.
    pub height: f64,
    /// Cells to lift the sprite off the ground, for art that carries its own
    /// footing.
    pub lift: f64,
    /// Mirror the art when the settler faces left. Off for art drawn facing the
    /// viewer, which should not flip at all.
    pub flip: bool,
    /// What was dropped, so the panel can say what it is showing.
    pub source: String,
}

impl Default for Clip {
    fn default() -> Self {
        Clip {
            w: 0,
            h: 0,
            px: Vec::new(),
            frames: 1,
            fps: 6.0,
            stride: false,
            height: 1.1,
            lift: 0.0,
            flip: true,
            source: String::new(),
        }
    }
}

impl Clip {
    /// A single image read as a row of equal frames.
    pub fn from_strip(w: i32, h: i32, px: Vec<u32>, frames: i32, source: String) -> Option<Clip> {
        if w <= 0 || h <= 0 || px.len() < (w * h) as usize {
            return None;
        }
        let frames = frames.clamp(1, MAX_FRAMES);
        let (w, h, px) = fit_sheet(w, h, &px, frames);
        Some(Clip { w, h, px, frames, source, ..Clip::default() })
    }

    /// One image per frame, laid into a sheet. Every frame is given the widest
    /// and tallest box any of them needs, centered across it and stood on its
    /// floor, so a set of images that are not quite the same size still lines
    /// up at the feet.
    pub fn from_frames(list: Vec<Frame>, source: String) -> Option<Clip> {
        let list: Vec<Frame> = list
            .into_iter()
            .filter(|(w, h, px)| *w > 0 && *h > 0 && px.len() >= (w * h) as usize)
            .take(MAX_FRAMES as usize)
            .collect();
        if list.is_empty() {
            return None;
        }
        let frames = list.len() as i32;
        let fw = list.iter().map(|f| f.0).max().unwrap_or(1);
        let fh = list.iter().map(|f| f.1).max().unwrap_or(1);
        let w = fw * frames;
        let mut px = vec![0u32; (w * fh) as usize];
        for (i, (sw, sh, sp)) in list.iter().enumerate() {
            let ox = i as i32 * fw + (fw - sw) / 2;
            let oy = fh - sh;
            for y in 0..*sh {
                for x in 0..*sw {
                    let v = sp[(y * sw + x) as usize];
                    if v == 0 {
                        continue;
                    }
                    px[((oy + y) * w + ox + x) as usize] = v;
                }
            }
        }
        let (w, h, px) = fit_sheet(w, fh, &px, frames);
        Some(Clip { w, h, px, frames, source, ..Clip::default() })
    }

    pub fn ready(&self) -> bool {
        self.w > 0 && self.h > 0 && self.px.len() >= (self.w * self.h) as usize
    }

    pub fn frame_count(&self) -> i32 {
        self.frames.clamp(1, MAX_FRAMES)
    }

    /// Width of one frame. The remainder of a sheet that does not divide evenly
    /// is left unread rather than stretching every frame to hide it.
    pub fn frame_w(&self) -> i32 {
        (self.w / self.frame_count()).max(1)
    }

    pub fn pixel(&self, frame: i32, x: i32, y: i32) -> u32 {
        let fw = self.frame_w();
        if x < 0 || y < 0 || x >= fw || y >= self.h {
            return 0;
        }
        let sx = frame.clamp(0, self.frame_count() - 1) * fw + x;
        if sx >= self.w {
            return 0;
        }
        self.px.get((y * self.w + sx) as usize).copied().unwrap_or(0)
    }

    /// Which frame is showing. `bob` counts six per cell walked, which is the
    /// cadence the generated settler stepped on, so a stride clip asked for six
    /// frames per second reads exactly as fast as the one it replaced.
    pub fn frame_index(&self, bob: f64, time: f64) -> i32 {
        let n = self.frame_count();
        let fps = self.fps.max(0.0);
        if n <= 1 || fps <= 0.0 {
            return 0;
        }
        let t = if self.stride { bob / 6.0 * fps } else { time * fps };
        if !t.is_finite() {
            return 0;
        }
        (t.floor() as i64).rem_euclid(n as i64) as i32
    }

    /// What the sheet costs in a saved project, where a pixel is eight hex
    /// characters.
    pub fn bytes(&self) -> usize {
        self.px.len() * 8
    }
}

/// Shrinks a sheet until one frame fits the cap, sampling nearest so pixel art
/// keeps its edges. The whole sheet is scaled by one ratio rather than frame by
/// frame, which keeps the proportions the frame count is read against.
fn fit_sheet(w: i32, h: i32, px: &[u32], frames: i32) -> (i32, i32, Vec<u32>) {
    let fw = (w / frames.max(1)).max(1);
    let ratio = (fw as f64 / MAX_FRAME_PX as f64).max(h as f64 / MAX_FRAME_PX as f64);
    if ratio <= 1.0 {
        return (w, h, px.to_vec());
    }
    let nw = ((w as f64 / ratio).round() as i32).max(frames.max(1));
    let nh = ((h as f64 / ratio).round() as i32).max(1);
    let mut out = vec![0u32; (nw * nh) as usize];
    for y in 0..nh {
        let sy = ((y as i64 * h as i64) / nh as i64).min(h as i64 - 1) as i32;
        for x in 0..nw {
            let sx = ((x as i64 * w as i64) / nw as i64).min(w as i64 - 1) as i32;
            out[(y * nw + x) as usize] = px[(sy * w + sx) as usize];
        }
    }
    (nw, nh, out)
}

/// How many frames a strip most likely holds. A sheet whose width is a whole
/// number of its height is read as that many square frames, which is how nearly
/// every strip is cut; anything else is one frame until it is told otherwise.
pub fn guess_frames(w: i32, h: i32) -> i32 {
    if w <= 0 || h <= 0 || w % h != 0 {
        return 1;
    }
    (w / h).clamp(1, MAX_FRAMES)
}

/// Every clip a settler can be drawn with.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PeopleSprites {
    /// Off draws the generated settler again without giving up the images.
    pub enabled: bool,
    pub idle: Option<Clip>,
    pub walk: Option<Clip>,
    pub carry: Option<Clip>,
    pub work: Option<Clip>,
    pub sleep: Option<Clip>,
    /// Bumped whenever a clip changes, so the drawing can tell a cached sprite
    /// built from the old pixels is stale. Not saved: a project that has only
    /// just been loaded has nothing cached to go stale.
    #[serde(skip)]
    pub rev: u32,
}

impl Default for PeopleSprites {
    fn default() -> Self {
        PeopleSprites {
            enabled: true,
            idle: None,
            walk: None,
            carry: None,
            work: None,
            sleep: None,
            rev: 0,
        }
    }
}

impl PeopleSprites {
    pub fn clip(&self, motion: Motion) -> Option<&Clip> {
        match motion {
            Motion::Idle => self.idle.as_ref(),
            Motion::Walk => self.walk.as_ref(),
            Motion::Carry => self.carry.as_ref(),
            Motion::Work => self.work.as_ref(),
            Motion::Sleep => self.sleep.as_ref(),
        }
    }

    pub fn slot_mut(&mut self, motion: Motion) -> &mut Option<Clip> {
        match motion {
            Motion::Idle => &mut self.idle,
            Motion::Walk => &mut self.walk,
            Motion::Carry => &mut self.carry,
            Motion::Work => &mut self.work,
            Motion::Sleep => &mut self.sleep,
        }
    }

    pub fn set(&mut self, motion: Motion, clip: Option<Clip>) {
        *self.slot_mut(motion) = clip;
        self.touch();
    }

    pub fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    /// The clip a motion is actually drawn with, and which slot it came from.
    /// Returns nothing when the images are switched off or when no clip in the
    /// fallback chain has any pixels, which is what sends the drawing back to
    /// the generated settler.
    pub fn resolve(&self, motion: Motion) -> Option<(Motion, &Clip)> {
        if !self.enabled {
            return None;
        }
        motion
            .chain()
            .iter()
            .find_map(|&m| self.clip(m).filter(|c| c.ready()).map(|c| (m, c)))
    }

    pub fn any(&self) -> bool {
        MOTIONS.iter().any(|&m| self.clip(m).is_some_and(|c| c.ready()))
    }

    /// What every sheet together costs in a saved project.
    pub fn bytes(&self) -> usize {
        MOTIONS.iter().filter_map(|&m| self.clip(m)).map(|c| c.bytes()).sum()
    }
}

/// What a settler is doing, folded down to the one thing the drawing asks.
/// Sleeping wins over everything, then being on a path, and only somebody
/// stood still and mid-task counts as working.
pub fn motion_of(p: &Person) -> Motion {
    if p.sleeping {
        return Motion::Sleep;
    }
    if !p.path.is_empty() {
        return if p.carrying() { Motion::Carry } else { Motion::Walk };
    }
    match &p.task {
        Some(task) if task.working() => Motion::Work,
        _ => Motion::Idle,
    }
}

/// Compares file names the way a numbered set of frames is meant to be read,
/// so `walk2` lands before `walk10`. A drop hands its files over in whatever
/// order the browser walked them, which is not the order they were named in.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0usize, 0usize);
    let number = |s: &[u8], at: &mut usize| -> u64 {
        let mut v = 0u64;
        while *at < s.len() && s[*at].is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add((s[*at] - b'0') as u64);
            *at += 1;
        }
        v
    };
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (na, nb) = (number(a, &mut i), number(b, &mut j));
            match na.cmp(&nb) {
                Ordering::Equal => {}
                other => return other,
            }
        } else {
            match a[i].to_ascii_lowercase().cmp(&b[j].to_ascii_lowercase()) {
                Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
                other => return other,
            }
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}
