// Shading model.
//
// Every plant pixel gets a tone value t in 0..1 (0 = darkest ramp entry,
// 1 = lightest) built from two inputs:
//
//   depth  0..1  how far inside its own shape the pixel sits (0 = silhouette
//                edge, 1 = core). Comes from a distance transform.
//   vert   0..1  vertical position inside that same shape (0 = top edge,
//                1 = bottom edge).
//
//   t = mid - centerDark * C(depth) + topLight * C(1 - vert) - bottomDark * C(vert)
//
// C() is the shared response curve: everything below edge0 reads as 0,
// everything above edge1 reads as 1, with a smoothstep between them raised to
// gamma. Pulling edge0 and edge1 close together gives a large flat plateau,
// which is what keeps the body of an object a single flat color and confines
// the shading to a rim.

export function defaultShading() {
  return {
    edge0: 0.12,
    edge1: 0.62,
    gamma: 1.0,
    mid: 0.55,
    centerDark: 0.42,
    topLight: 0.34,
    bottomDark: 0.3,
  };
}

export function curveValue(x, s) {
  const span = Math.max(1e-6, s.edge1 - s.edge0);
  let t = (x - s.edge0) / span;
  t = t < 0 ? 0 : t > 1 ? 1 : t;
  t = t * t * (3 - 2 * t);
  return s.gamma === 1 ? t : Math.pow(t, s.gamma);
}

export function shadeValue(depth, vert, s) {
  let t = s.mid;
  t -= s.centerDark * curveValue(depth, s);
  t += s.topLight * curveValue(1 - vert, s);
  t -= s.bottomDark * curveValue(vert, s);
  return t < 0 ? 0 : t > 1 ? 1 : t;
}

// Snap to a fixed number of tones so output stays readable as pixel art.
export function quantize(t, tones) {
  if (!tones || tones < 2) return t;
  const n = tones - 1;
  return Math.round(t * n) / n;
}
