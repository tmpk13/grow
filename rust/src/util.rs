//! Small numeric / color / raster helpers shared by the whole tool.
//!
//! Colors are stored packed in a u32 laid out the way a little endian machine
//! reads an RGBA byte quad, so a sprite can be blitted into an image buffer
//! with a plain copy. WebAssembly is always little endian.

pub const EMPTY_COLOR: u32 = 0;

pub fn clamp(v: f64, a: f64, b: f64) -> f64 {
    if v < a {
        a
    } else if v > b {
        b
    } else {
        v
    }
}

pub fn clamp01(v: f64) -> f64 {
    clamp(v, 0.0, 1.0)
}

pub fn clampi(v: i32, a: i32, b: i32) -> i32 {
    if v < a {
        a
    } else if v > b {
        b
    } else {
        v
    }
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

pub fn to_rad(deg: f64) -> f64 {
    deg * std::f64::consts::PI / 180.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

pub const fn pack_rgba(r: i32, g: i32, b: i32, a: i32) -> u32 {
    (((a & 255) as u32) << 24) | (((b & 255) as u32) << 16) | (((g & 255) as u32) << 8) | ((r & 255) as u32)
}

pub const fn unpack_rgba(v: u32) -> Rgba {
    Rgba {
        r: (v & 255) as u8,
        g: ((v >> 8) & 255) as u8,
        b: ((v >> 16) & 255) as u8,
        a: ((v >> 24) & 255) as u8,
    }
}

pub fn hex_to_packed(hex: &str) -> u32 {
    hex_to_packed_alpha(hex, 255)
}

pub fn hex_to_packed_alpha(hex: &str, alpha: i32) -> u32 {
    let trimmed = hex.trim().trim_start_matches('#');
    let s: String = if trimmed.len() == 3 {
        trimmed.chars().flat_map(|c| [c, c]).collect()
    } else {
        trimmed.to_string()
    };
    let byte = |at: usize| -> i32 {
        s.get(at..at + 2)
            .and_then(|part| i32::from_str_radix(part, 16).ok())
            .unwrap_or(0)
    };
    let a = if s.len() >= 8 { byte(6) } else { alpha };
    pack_rgba(byte(0), byte(2), byte(4), a)
}

pub fn packed_to_hex(v: u32) -> String {
    let c = unpack_rgba(v);
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

pub fn packed_to_rgba_hex(v: u32) -> String {
    let c = unpack_rgba(v);
    format!("{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
}

pub fn rgba_hex_to_packed(s: &str) -> u32 {
    let byte = |at: usize| -> i32 {
        s.get(at..at + 2)
            .and_then(|part| i32::from_str_radix(part, 16).ok())
            .unwrap_or(0)
    };
    pack_rgba(byte(0), byte(2), byte(4), byte(6))
}

pub fn luminance(v: u32) -> f64 {
    let c = unpack_rgba(v);
    0.2126 * c.r as f64 + 0.7152 * c.g as f64 + 0.0722 * c.b as f64
}

pub fn mix_packed(a: u32, b: u32, t: f64) -> u32 {
    let ca = unpack_rgba(a);
    let cb = unpack_rgba(b);
    pack_rgba(
        lerp(ca.r as f64, cb.r as f64, t).round() as i32,
        lerp(ca.g as f64, cb.g as f64, t).round() as i32,
        lerp(ca.b as f64, cb.b as f64, t).round() as i32,
        lerp(ca.a as f64, cb.a as f64, t).round() as i32,
    )
}

pub fn hsl_to_packed(h: f64, s: f64, l: f64) -> u32 {
    let hh = (h % 360.0 + 360.0) % 360.0 / 360.0;
    let ss = clamp01(s);
    let ll = clamp01(l);
    let q = if ll < 0.5 { ll * (1.0 + ss) } else { ll + ss - ll * ss };
    let p = 2.0 * ll - q;
    let chan = |t: f64| -> f64 {
        let mut x = t;
        if x < 0.0 {
            x += 1.0;
        }
        if x > 1.0 {
            x -= 1.0;
        }
        if x < 1.0 / 6.0 {
            p + (q - p) * 6.0 * x
        } else if x < 1.0 / 2.0 {
            q
        } else if x < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - x) * 6.0
        } else {
            p
        }
    };
    pack_rgba(
        (chan(hh + 1.0 / 3.0) * 255.0).round() as i32,
        (chan(hh) * 255.0).round() as i32,
        (chan(hh - 1.0 / 3.0) * 255.0).round() as i32,
        255,
    )
}

/// Stable value noise in [0,1). Used for blob shapes and per-pixel jitter so a
/// re-raster of the same plant produces identical pixels.
pub fn hash2(x: i32, y: i32, seed: i32) -> f64 {
    // The mixing is done the way the original tool did it: the first sum is a
    // plain multiply-add that may exceed 32 bits before being folded back, and
    // only then does the wrapping mix start.
    let sum = x as f64 * 374761393.0 + y as f64 * 668265263.0 + seed as f64 * 1442695041.0;
    let h = to_u32(sum);
    let h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) as f64) / 4294967296.0
}

/// Folds a float into the low 32 bits, truncating toward zero.
fn to_u32(f: f64) -> u32 {
    if !f.is_finite() {
        return 0;
    }
    let m = f.trunc().rem_euclid(4294967296.0);
    m as u32
}

/// Chamfer 3-4 distance transform: distance in pixels from every non-zero mask
/// pixel to the nearest zero pixel. Out of bounds counts as zero, so a shape
/// touching the buffer edge is treated as ending there.
pub fn distance_transform(mask: &[u8], w: usize, h: usize, out: &mut Vec<f32>) {
    out.clear();
    out.resize(w * h, 0.0);
    const INF: f32 = 1e9;
    const A: f32 = 3.0;
    const B: f32 = 4.0;
    for i in 0..w * h {
        out[i] = if mask[i] != 0 { INF } else { 0.0 };
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if out[i] == 0.0 {
                continue;
            }
            let mut best = out[i];
            if y > 0 {
                if x > 0 {
                    best = best.min(out[i - w - 1] + B);
                }
                best = best.min(out[i - w] + A);
                if x < w - 1 {
                    best = best.min(out[i - w + 1] + B);
                }
            } else {
                best = best.min(A);
            }
            if x > 0 {
                best = best.min(out[i - 1] + A);
            } else {
                best = best.min(A);
            }
            if x == w - 1 {
                best = best.min(A);
            }
            out[i] = best;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if out[i] == 0.0 {
                continue;
            }
            let mut best = out[i];
            if y < h - 1 {
                if x < w - 1 {
                    best = best.min(out[i + w + 1] + B);
                }
                best = best.min(out[i + w] + A);
                if x > 0 {
                    best = best.min(out[i + w - 1] + B);
                }
            } else {
                best = best.min(A);
            }
            if x < w - 1 {
                best = best.min(out[i + 1] + A);
            } else {
                best = best.min(A);
            }
            out[i] = best;
        }
    }
    for v in out.iter_mut() {
        *v /= 3.0;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Component {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub max_depth: f32,
    pub count: u32,
}

/// 4-connected labelling. Fills `labels` with -1 for background and a component
/// index otherwise, and returns a bounding box per component; the caller fills
/// in `max_depth` from the distance transform.
pub fn label_components(
    mask: &[u8],
    w: usize,
    h: usize,
    labels: &mut Vec<i32>,
    stack: &mut Vec<usize>,
) -> Vec<Component> {
    labels.clear();
    labels.resize(w * h, -1);
    let mut comps: Vec<Component> = Vec::new();
    for seed in 0..w * h {
        if mask[seed] == 0 || labels[seed] != -1 {
            continue;
        }
        let id = comps.len() as i32;
        let mut comp = Component {
            x0: w as i32,
            y0: h as i32,
            x1: 0,
            y1: 0,
            max_depth: 0.0,
            count: 0,
        };
        stack.clear();
        stack.push(seed);
        labels[seed] = id;
        while let Some(i) = stack.pop() {
            let x = (i % w) as i32;
            let y = (i / w) as i32;
            if x < comp.x0 {
                comp.x0 = x;
            }
            if x > comp.x1 {
                comp.x1 = x;
            }
            if y < comp.y0 {
                comp.y0 = y;
            }
            if y > comp.y1 {
                comp.y1 = y;
            }
            comp.count += 1;
            if x > 0 && mask[i - 1] != 0 && labels[i - 1] == -1 {
                labels[i - 1] = id;
                stack.push(i - 1);
            }
            if (x as usize) < w - 1 && mask[i + 1] != 0 && labels[i + 1] == -1 {
                labels[i + 1] = id;
                stack.push(i + 1);
            }
            if y > 0 && mask[i - w] != 0 && labels[i - w] == -1 {
                labels[i - w] = id;
                stack.push(i - w);
            }
            if (y as usize) < h - 1 && mask[i + w] != 0 && labels[i + w] == -1 {
                labels[i + w] = id;
                stack.push(i + w);
            }
        }
        comps.push(comp);
    }
    comps
}

/// Short unique-enough id for a thing the user just created.
pub fn uid(prefix: &str, n: u32) -> String {
    format!("{prefix}-{n:06x}")
}

/// Hue in degrees, saturation and value in 0..1, to a packed opaque color.
pub fn hsv_to_packed(h: f64, s: f64, v: f64) -> u32 {
    let hh = (h % 360.0 + 360.0) % 360.0 / 60.0;
    let s = clamp01(s);
    let v = clamp01(v);
    let c = v * s;
    let x = c * (1.0 - ((hh % 2.0) - 1.0).abs());
    let (r, g, b) = match hh as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    pack_rgba(
        ((r + m) * 255.0).round() as i32,
        ((g + m) * 255.0).round() as i32,
        ((b + m) * 255.0).round() as i32,
        255,
    )
}

/// The inverse, for seeding a wheel from a color that came from somewhere else.
/// A gray has no hue to recover, so the caller's current hue is kept instead of
/// snapping the wheel back to red.
pub fn packed_to_hsv(v: u32, keep_hue: f64) -> (f64, f64, f64) {
    let c = unpack_rgba(v);
    let (r, g, b) = (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let hue = if d <= 0.0 {
        keep_hue
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let hue = (hue % 360.0 + 360.0) % 360.0;
    let sat = if max <= 0.0 { 0.0 } else { d / max };
    (hue, sat, max)
}
