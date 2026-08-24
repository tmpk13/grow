//! Shading model.
//!
//! Every plant pixel gets a tone value t in 0..1 (0 = darkest ramp entry,
//! 1 = lightest) built from two inputs:
//!
//!   depth  0..1  how far inside its own shape the pixel sits (0 = silhouette
//!                edge, 1 = core). Comes from a distance transform.
//!   vert   0..1  vertical position inside that same shape (0 = top edge,
//!                1 = bottom edge).
//!
//!   t = mid - center_dark * C(depth) + top_light * C(1 - vert) - bottom_dark * C(vert)
//!
//! C() is the shared response curve: everything below edge0 reads as 0,
//! everything above edge1 reads as 1, with a smoothstep between them raised to
//! gamma. Pulling edge0 and edge1 close together gives a large flat plateau,
//! which is what keeps the body of an object a single flat color and confines
//! the shading to a rim.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Shading {
    pub edge0: f64,
    pub edge1: f64,
    pub gamma: f64,
    pub mid: f64,
    pub center_dark: f64,
    pub top_light: f64,
    pub bottom_dark: f64,
}

impl Default for Shading {
    fn default() -> Self {
        Shading {
            edge0: 0.12,
            edge1: 0.62,
            gamma: 1.0,
            mid: 0.55,
            center_dark: 0.42,
            top_light: 0.34,
            bottom_dark: 0.3,
        }
    }
}

pub fn curve_value(x: f64, s: &Shading) -> f64 {
    let span = (s.edge1 - s.edge0).max(1e-6);
    let mut t = (x - s.edge0) / span;
    t = t.clamp(0.0, 1.0);
    t = t * t * (3.0 - 2.0 * t);
    if s.gamma == 1.0 {
        t
    } else {
        t.powf(s.gamma)
    }
}

pub fn shade_value(depth: f64, vert: f64, s: &Shading) -> f64 {
    let mut t = s.mid;
    t -= s.center_dark * curve_value(depth, s);
    t += s.top_light * curve_value(1.0 - vert, s);
    t -= s.bottom_dark * curve_value(vert, s);
    t.clamp(0.0, 1.0)
}

/// Snap to a fixed number of tones so output stays readable as pixel art.
pub fn quantize(t: f64, tones: i32) -> f64 {
    if tones < 2 {
        return t;
    }
    let n = (tones - 1) as f64;
    (t * n).round() / n
}
