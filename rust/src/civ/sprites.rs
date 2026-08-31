//! Settler animations built from dropped images.
//!
//! A clip is one sheet of pixels read as a row of equal frames, plus how it is
//! played. The sheet is kept whole rather than cut up, so changing the frame
//! count re-reads the same image instead of asking for it to be dropped again.
//!
//! How large a clip comes out is the art's own business: a source pixel stands
//! for a fixed fraction of a cell, so a frame is drawn at the size it was drawn
//! at, in proportion, and two motions exported from the same canvas match
//! without either of them being measured.
//!
//! There is one clip per motion, and a motion with nothing dropped on it falls
//! back to a related one, so a single walk sheet is enough to stand in for the
//! generated settler everywhere.

use serde::{Deserialize, Serialize};

use crate::civ::people::Person;

/// A clip's pixels: tagged runs on the way out, and either those or the plain
/// pixel-per-hex-quad form clips were written in before on the way in.
mod px_art {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(px: &[u32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::art::encode_runs(px))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u32>, D::Error> {
        let raw = String::deserialize(d)?;
        if let Some(px) = crate::art::decode_runs(&raw) {
            return Ok(px);
        }
        let mut out = Vec::with_capacity(raw.len() / 8);
        let mut at = 0;
        while at + 8 <= raw.len() {
            out.push(crate::util::rgba_hex_to_packed(&raw[at..at + 8]));
            at += 8;
        }
        Ok(out)
    }
}

/// Frames a clip may hold, and the largest one frame may be. Both are here to
/// keep a project that lives in local storage from being filled by a
/// screenshot dropped on the panel by mistake; a sheet past either is scaled
/// down or cut short on the way in. A frame that had to be scaled down keeps
/// the size it was meant to be drawn at through the clip's `scale`.
pub const MAX_FRAMES: i32 = 24;
pub const MAX_FRAME_PX: i32 = 256;

/// Art pixels to a map cell, when a project has not said otherwise. Eight is
/// the cell width a settlement starts at, so art dropped on a fresh project is
/// drawn at its own pixel size.
pub const DEFAULT_ART_PX_PER_CELL: f64 = 8.0;

/// What a clip's scale may be set to. Under a twentieth nothing would be left
/// of the art, and past sixteen a frame is a wall.
pub const MIN_SCALE: f64 = 0.05;
pub const MAX_SCALE: f64 = 16.0;

/// The largest a frame is drawn, in cells either way. A picture past this is a
/// mistake rather than a decision, and the blit is what would pay for it. Both
/// sides are held to the same ratio, so hitting it shrinks the art rather than
/// squashing it.
pub const MAX_DRAWN_CELLS: i32 = 32;

/// Map pixels one source pixel covers.
pub fn art_zoom(cell_px: i32, px_per_cell: f64, scale: f64) -> f64 {
    let per_cell = if px_per_cell.is_finite() { px_per_cell.clamp(1.0, 256.0) } else { DEFAULT_ART_PX_PER_CELL };
    let scale = if scale.is_finite() { scale.clamp(MIN_SCALE, MAX_SCALE) } else { 1.0 };
    cell_px.max(1) as f64 / per_cell * scale
}

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
    Swim,
    ToBed,
    /// In the water and going nowhere: treading rather than crossing.
    Float,
    /// Off the ground in somebody's hand, which is the one motion the
    /// simulation never puts anybody in: it is a person doing the lifting.
    Held,
}

pub const MOTIONS: [Motion; 9] = [
    Motion::Idle,
    Motion::Walk,
    Motion::Carry,
    Motion::Work,
    Motion::ToBed,
    Motion::Sleep,
    Motion::Swim,
    Motion::Float,
    Motion::Held,
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
            Motion::Swim => "Swimming",
            Motion::Float => "Still in water",
            Motion::Held => "Picked up",
            Motion::ToBed => "Going to sleep",
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
            Motion::Swim => "swim",
            Motion::Float => "float",
            Motion::Held => "held",
            Motion::ToBed => "tobed",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Motion::Idle => "on their feet with nowhere to be",
            Motion::Walk => "on a path, hands empty",
            Motion::Carry => "on a path with a load",
            Motion::Work => "stood at the work rather than walking to it",
            Motion::Sleep => "asleep out in the open",
            Motion::Swim => "in the water, crossing it",
            Motion::Float => "in the water with nowhere to be, treading it",
            Motion::Held => "off the ground, in hand, while somebody moves them",
            Motion::ToBed => "turning in: on the way to a bed, or lying down in the open",
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
            Motion::Swim => (4.0, true),
            Motion::Float => (2.0, false),
            // Nothing about being carried is tied to the ground: the ground is
            // what they are not on.
            Motion::Held => (3.0, false),
            Motion::ToBed => (4.0, true),
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
            // A swim falls back to treading, and only then to a walk: a walk
            // borrowed for the water is cut at the waterline, which reads as
            // wading rather than as standing on it, but art drawn for the water
            // is the better answer and comes first.
            Motion::Swim => &[Motion::Swim, Motion::Float, Motion::Walk, Motion::Idle],
            Motion::Float => &[Motion::Float, Motion::Swim, Motion::Idle, Motion::Walk],
            // Being carried falls back to standing: dangling from a hand is
            // nothing like a walk cycle, and a stand at least does not stride
            // through the air.
            Motion::Held => &[Motion::Held, Motion::Idle],
            // Turning in is a walk until somebody draws it otherwise, and a
            // stand when there is nowhere to walk to.
            Motion::ToBed => &[Motion::ToBed, Motion::Walk, Motion::Idle],
        }
    }
}

/// One image on its way into a sheet: width, height and packed pixels.
pub type Frame = (i32, i32, Vec<u32>);

/// Where a motion's art came from, and whether it is still what the editor has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FromSheet {
    /// Dropped in as files, so there is no sheet behind it to be behind.
    Dropped,
    /// Taken from a sheet that has not been touched since.
    Current,
    /// Taken from a sheet that has been drawn on since; taking it again would
    /// change what is on the map.
    Behind,
    /// Taken from a sheet that is not in the project any more.
    Gone,
}

/// One animation: a sheet of frames laid left to right, and how it is played.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Clip {
    /// The sheet, at whatever size it arrived, scaled only if a frame was over
    /// the cap. Frames are read out of it by dividing, which is what lets the
    /// count be changed after the drop.
    pub w: i32,
    pub h: i32,
    #[serde(with = "px_art")]
    pub px: Vec<u32>,
    pub frames: i32,
    /// Frames per second, or frames per cell walked when `stride` is set.
    pub fps: f64,
    /// Tie the frame to ground covered rather than to the clock, so a walk
    /// never slides and never runs on the spot.
    pub stride: bool,
    /// Multiplies the size the art comes out at. One is the art's own size:
    /// every source pixel covers what one art pixel is worth, which is what the
    /// project's art pixels per cell says. A frame that had to be scaled down
    /// to fit the cap arrives with the scale that puts it back.
    pub scale: f64,
    /// What this was drawn at, in cells, before art carried its own size. Read
    /// from an older project and turned into a scale on the way in; never
    /// written, so it leaves the file the first time the project is saved.
    #[serde(skip_serializing)]
    pub height: f64,
    /// Cells to lift the sprite off the ground, for art that carries its own
    /// footing.
    pub lift: f64,
    /// Mirror the art when the settler faces left. Off for art drawn facing the
    /// viewer, which should not flip at all.
    pub flip: bool,
    /// Mirror the sheet itself, for art drawn facing the other way than the
    /// settler it stands in for. Read on the way out rather than baked in, so
    /// it can be turned off again without dropping the images a second time.
    pub mirror: bool,
    /// What was dropped, so the panel can say what it is showing.
    pub source: String,
    /// The sheet this was built from, when it came from the editor. The clip
    /// is a copy and stays one; this is only so the panel can offer to build it
    /// again from a sheet that has moved on since.
    pub sheet: String,
    /// The sheet's fingerprint at the moment it was taken. Comparing it with
    /// the sheet's fingerprint now is what says whether taking it again would
    /// change anything.
    #[serde(default)]
    pub stamp: String,
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
            scale: 1.0,
            height: 0.0,
            lift: 0.0,
            flip: true,
            mirror: false,
            source: String::new(),
            sheet: String::new(),
            stamp: String::new(),
        }
    }
}

impl Clip {
    /// A single image read as a row of equal frames. The frame is kept as it
    /// was drawn, padding and all: where the art sits in it is the composition,
    /// and it is what puts a figure's feet on the ground and holds a tool out
    /// to the side of them.
    pub fn from_strip(w: i32, h: i32, px: Vec<u32>, frames: i32, source: String) -> Option<Clip> {
        if w <= 0 || h <= 0 || px.len() < (w * h) as usize {
            return None;
        }
        let frames = frames.clamp(1, MAX_FRAMES);
        let (w, h, px, scale) = fit_sheet(w, h, &px, frames);
        Some(Clip { w, h, px, frames, source, scale, ..Clip::default() })
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
        let (w, h, px, scale) = fit_sheet(w, fh, &px, frames);
        Some(Clip { w, h, px, frames, source, scale, ..Clip::default() })
    }

    /// A clip built from a sheet drawn in the editor. The sheet is already a
    /// row of equal frames, so it goes in whole rather than being guessed at,
    /// and the sheet's own rate comes with it.
    pub fn from_sheet(sheet: &crate::art::Sheet) -> Option<Clip> {
        let (w, h, px) = sheet.strip();
        let frames = sheet.frame_count().clamp(1, MAX_FRAMES);
        if w <= 0 || h <= 0 || px.iter().all(|v| *v == 0) {
            return None;
        }
        let (w, h, px, scale) = fit_sheet(w, h, &px, frames);
        Some(Clip {
            w,
            h,
            px,
            frames,
            scale,
            fps: sheet.fps,
            source: format!("editor: {}", sheet.name),
            sheet: sheet.id.clone(),
            stamp: sheet.stamp(),
            ..Clip::default()
        })
    }

    /// How this clip stands against the sheet it came from.
    pub fn against(&self, sheet: Option<&crate::art::Sheet>) -> FromSheet {
        if self.sheet.is_empty() {
            return FromSheet::Dropped;
        }
        match sheet {
            None => FromSheet::Gone,
            Some(s) if s.stamp() == self.stamp => FromSheet::Current,
            Some(_) => FromSheet::Behind,
        }
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

    /// Where a frame starts in the sheet. Cut from the full width rather than
    /// by stepping the floored frame width, so a sheet that does not divide
    /// evenly loses at most a column at the end instead of sliding a little
    /// further off true with every frame.
    fn frame_start(&self, frame: i32) -> i32 {
        let n = self.frame_count();
        let frame = frame.clamp(0, n - 1);
        let start = (frame as i64 * self.w as i64 / n as i64) as i32;
        start.min(self.w - self.frame_w()).max(0)
    }

    pub fn pixel(&self, frame: i32, x: i32, y: i32) -> u32 {
        let fw = self.frame_w();
        if x < 0 || y < 0 || x >= fw || y >= self.h {
            return 0;
        }
        let x = if self.mirror { fw - 1 - x } else { x };
        let sx = self.frame_start(frame) + x;
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

    /// Turns a height in cells, which is how a clip was sized before art
    /// carried its own, into the scale that draws it at that same height, and
    /// forgets it. A clip written since has no height on it and is left alone.
    pub fn take_legacy_height(&mut self, px_per_cell: f64) {
        let height = std::mem::take(&mut self.height);
        if height <= 0.0 || self.h <= 0 {
            return;
        }
        let per_cell = if px_per_cell.is_finite() {
            px_per_cell.clamp(1.0, 256.0)
        } else {
            DEFAULT_ART_PX_PER_CELL
        };
        self.scale = (height * per_cell / self.h as f64).clamp(MIN_SCALE, MAX_SCALE);
    }

    /// How large one frame comes out on the map, in map pixels. The art's own
    /// pixels are what says it: nothing here is measured against a box, so the
    /// same source at the same scale is the same size in every slot it is
    /// dropped on, and the shape of the frame is never touched.
    pub fn drawn_size(&self, cell_px: i32, px_per_cell: f64) -> (i32, i32) {
        let zoom = art_zoom(cell_px, px_per_cell, self.scale);
        let (w, h) = (self.frame_w() as f64 * zoom, self.h as f64 * zoom);
        let cap = (cell_px.max(1) * MAX_DRAWN_CELLS) as f64;
        let over = (w / cap).max(h / cap).max(1.0);
        (
            ((w / over).round() as i32).max(1),
            ((h / over).round() as i32).max(1),
        )
    }

    /// The same size read in cells, which is what the panel says out loud: a
    /// settler stands about a cell and a bit, a house is two or three across.
    pub fn drawn_cells(&self, cell_px: i32, px_per_cell: f64) -> (f64, f64) {
        let (w, h) = self.drawn_size(cell_px, px_per_cell);
        let cell = cell_px.max(1) as f64;
        (w as f64 / cell, h as f64 / cell)
    }

    /// What the sheet costs in a saved project: twelve characters a run.
    pub fn bytes(&self) -> usize {
        crate::art::run_count(&self.px) * 12
    }
}

/// Shrinks a sheet until one frame fits the cap, sampling nearest so pixel art
/// keeps its edges. The whole sheet is scaled by one ratio rather than frame by
/// frame, which keeps the proportions the frame count is read against, and the
/// scaled width is held to a whole number of frames so the sheet still divides
/// exactly afterwards.
///
/// The ratio comes back with it as the scale that puts the loss back: the sheet
/// holds fewer pixels than it was drawn with, but it is still drawn at the size
/// those pixels were meant to cover.
fn fit_sheet(w: i32, h: i32, px: &[u32], frames: i32) -> (i32, i32, Vec<u32>, f64) {
    let frames = frames.max(1);
    let fw = (w / frames).max(1);
    let ratio = (fw as f64 / MAX_FRAME_PX as f64).max(h as f64 / MAX_FRAME_PX as f64);
    if ratio <= 1.0 {
        return (w, h, px.to_vec(), 1.0);
    }
    let nfw = ((fw as f64 / ratio).round() as i32).max(1);
    let nw = nfw * frames;
    let nh = ((h as f64 / ratio).round() as i32).max(1);
    let mut out = vec![0u32; (nw * nh) as usize];
    for y in 0..nh {
        let sy = ((y as i64 * h as i64) / nh as i64).min(h as i64 - 1) as i32;
        for f in 0..frames {
            // Every frame is read from its own span of the source, so the
            // rounding that fits the cap cannot walk the sample window off the
            // frame it belongs to.
            let s0 = (f as i64 * w as i64 / frames as i64) as i32;
            let s1 = ((f + 1) as i64 * w as i64 / frames as i64) as i32;
            let span = (s1 - s0).max(1);
            for x in 0..nfw {
                let sx = (s0 + ((x as i64 * span as i64) / nfw as i64) as i32).min(w - 1);
                out[(y * nw + f * nfw + x) as usize] = px[(sy * w + sx) as usize];
            }
        }
    }
    (nw, nh, out, ratio.clamp(MIN_SCALE, MAX_SCALE))
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
    pub swim: Option<Clip>,
    /// Treading water, which is what somebody in the water with nothing to do
    /// is doing.
    pub float: Option<Clip>,
    /// Held in hand, off the ground and going where the pointer goes.
    pub held: Option<Clip>,
    pub to_bed: Option<Clip>,
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
            swim: None,
            float: None,
            held: None,
            to_bed: None,
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
            Motion::Swim => self.swim.as_ref(),
            Motion::Float => self.float.as_ref(),
            Motion::Held => self.held.as_ref(),
            Motion::ToBed => self.to_bed.as_ref(),
        }
    }

    pub fn slot_mut(&mut self, motion: Motion) -> &mut Option<Clip> {
        match motion {
            Motion::Idle => &mut self.idle,
            Motion::Walk => &mut self.walk,
            Motion::Carry => &mut self.carry,
            Motion::Work => &mut self.work,
            Motion::Sleep => &mut self.sleep,
            Motion::Swim => &mut self.swim,
            Motion::Float => &mut self.float,
            Motion::Held => &mut self.held,
            Motion::ToBed => &mut self.to_bed,
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
/// Sleeping wins over everything, then being in the water, then turning in for
/// the night, then being on a path, and only somebody stood still and mid-task
/// counts as working.
/// Whether art about to be drawn in the water has to be cut at the waterline.
///
/// Art drawn for the water draws its own: a swimmer is a head and a wake, and
/// cutting it would take the wake off along with the rest. Anything borrowed
/// from dry land is a standing figure, and the cut is what puts it in the water
/// rather than on it. Nothing at all is the generated settler, which has a
/// water pose of its own.
pub fn cut_at_waterline(drawn: Option<Motion>) -> bool {
    matches!(drawn, Some(m) if !matches!(m, Motion::Swim | Motion::Float))
}

pub fn motion_of(p: &Person, swimming: bool, held: bool) -> Motion {
    // Being in hand beats everything, including sleep: whoever is holding them
    // has taken them out of whatever they were doing.
    if held {
        return Motion::Held;
    }
    if p.sleeping {
        return Motion::Sleep;
    }
    // Being in the water beats what is being carried through it. Somebody
    // crossing it is swimming; somebody in it with nowhere to be is treading,
    // which is a pose rather than a stroke.
    if swimming {
        return if p.path.is_empty() { Motion::Float } else { Motion::Swim };
    }
    // Turning in is its own thing, whether that is the walk to a bed or lying
    // down where they stand: the settler has finished for the day either way,
    // which a walk cycle does not say.
    if p.task.as_ref().is_some_and(|t| t.is_sleep()) {
        return Motion::ToBed;
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

// ---- the things people make ---------------------------------------------

/// One thing that can be given a picture instead of being generated from the
/// sampling boxes: a building, a boat, or a load in somebody's hands.
pub struct MadeSlot {
    /// What the thing is called in the catalog. Keyed by name rather than by
    /// position, so adding a building does not move what the others point at.
    pub id: String,
    pub label: String,
    pub group: &'static str,
}

/// Everything with a picture slot, in the order the panel lists them.
pub fn made_slots() -> Vec<MadeSlot> {
    let mut out: Vec<MadeSlot> = crate::civ::buildings::BUILDINGS
        .iter()
        .map(|def| MadeSlot {
            id: def.id.to_string(),
            label: def.label.to_string(),
            group: def.category.label(),
        })
        .collect();
    out.push(MadeSlot { id: "boat".into(), label: "Boat".into(), group: "On the water" });
    for res in crate::civ::resources::RES_IDS {
        out.push(MadeSlot {
            id: format!("carry-{}", res.id()),
            label: format!("{} in hand", res.label()),
            group: "Carried",
        });
    }
    out
}

/// A state a made thing can be in, and what to call it. The first is the one
/// everything falls back to, which is why it has no name of its own.
pub type MadeState = (&'static str, &'static str);

const ALWAYS: MadeState = ("", "Always");

/// Every state a building can be drawn in. Generic on purpose: a state that
/// means nothing for a given building simply never comes up, and a slot with
/// no picture in it falls back to the one that does.
const BUILDING_STATES: [MadeState; 4] = [
    ALWAYS,
    ("site", "Going up"),
    ("working", "With somebody at it"),
    ("night", "After dark"),
];

const BOAT_STATES: [MadeState; 2] = [ALWAYS, ("laden", "Carrying cargo")];

const ONE_STATE: [MadeState; 1] = [ALWAYS];

/// The states a slot can be given a picture for.
pub fn made_states(id: &str) -> &'static [MadeState] {
    if id == "boat" {
        &BOAT_STATES
    } else if id.starts_with("carry-") {
        &ONE_STATE
    } else {
        &BUILDING_STATES
    }
}

/// How a picture is keyed: the thing, and the state when it is not the one
/// everything falls back to.
pub fn made_key(id: &str, state: &str) -> String {
    if state.is_empty() {
        id.to_string()
    } else {
        format!("{id}:{state}")
    }
}

/// Every thing and state as something the menu ranker can search, so the
/// picture panel gets the same fuzzy and meaning matching the menus do.
///
/// Here rather than in the panel because the tool that builds the meaning
/// table has to produce exactly this list, and a list written twice is a list
/// that will differ.
pub fn made_entries() -> Vec<crate::find::Entry> {
    let mut entries = Vec::new();
    for slot in made_slots() {
        for (state, said) in made_states(&slot.id) {
            entries.push(crate::find::Entry {
                mode: "settlement".into(),
                mode_label: "Settlement".into(),
                tab: "build".into(),
                tab_label: "Build".into(),
                group: slot.group.to_string(),
                label: if state.is_empty() {
                    slot.label.clone()
                } else {
                    format!("{} - {}", slot.label, said.to_lowercase())
                },
                hint: format!(
                    "a picture for {} {}",
                    slot.label.to_lowercase(),
                    said.to_lowercase()
                ),
                anchor: made_key(&slot.id, state),
                kind: "made".into(),
            });
        }
    }
    entries
}

/// Pictures for the things people make. A settler has a clip per motion; a
/// building has one picture, drawn at the size the generator would have drawn
/// it, so art and generated things stand together on the same map.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MadeSprites {
    pub enabled: bool,
    pub slots: std::collections::BTreeMap<String, Clip>,
    /// Bumped whenever a picture changes, so the sprite cache lets go of what
    /// it drew from the old one. Not part of the project.
    #[serde(skip)]
    pub rev: u32,
}

impl MadeSprites {
    /// The picture for a thing, if there is one and it is being used.
    pub fn clip(&self, id: &str) -> Option<&Clip> {
        if !self.enabled {
            return None;
        }
        self.slots.get(id).filter(|c| c.ready())
    }

    /// The picture for a thing in a state, falling back to the one it is drawn
    /// in the rest of the time. A thing with a picture for After dark and none
    /// for anything else is drawn from it after dark and generated by day,
    /// which is the same rule settler motions follow.
    pub fn clip_in(&self, id: &str, state: &str) -> Option<&Clip> {
        if !self.enabled {
            return None;
        }
        self.slots
            .get(&made_key(id, state))
            .filter(|c| c.ready())
            .or_else(|| self.slots.get(id).filter(|c| c.ready()))
    }

    /// Every state of every thing that has a picture, for the panel to count.
    pub fn filled(&self, id: &str) -> usize {
        made_states(id)
            .iter()
            .filter(|(state, _)| {
                self.slots.get(&made_key(id, state)).is_some_and(|c| c.ready())
            })
            .count()
    }

    /// The picture whether or not it is being used, which is what the panel
    /// shows.
    pub fn slot(&self, id: &str) -> Option<&Clip> {
        self.slots.get(id)
    }

    /// The picture for one key exactly, with no falling back, and only when
    /// pictures are being drawn at all.
    pub fn slot_ready(&self, key: &str) -> Option<&Clip> {
        if !self.enabled {
            return None;
        }
        self.slots.get(key).filter(|c| c.ready())
    }

    pub fn set(&mut self, id: &str, clip: Clip) {
        self.slots.insert(id.to_string(), clip);
        self.touch();
    }

    /// The picture for one key, to be changed in place. Whoever changes it owes
    /// the drawing a `touch`, which is what lets go of the sprite built from
    /// what it used to say.
    pub fn slot_mut(&mut self, id: &str) -> Option<&mut Clip> {
        self.slots.get_mut(id)
    }

    pub fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    pub fn clear(&mut self, id: &str) {
        self.slots.remove(id);
        self.rev = self.rev.wrapping_add(1);
    }

    pub fn bytes(&self) -> usize {
        self.slots.values().map(|c| c.px.len() * 4).sum()
    }
}
