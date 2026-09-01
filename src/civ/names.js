// Procedural names. People, their settlement and the caravans that visit are
// all named from the same syllable tables so a run reads as one place.

const STARTS = ['b', 'br', 'd', 'f', 'g', 'gr', 'h', 'k', 'l', 'm', 'n', 'r', 's', 'st', 't', 'th', 'v', 'w'];
const VOWELS = ['a', 'e', 'i', 'o', 'u', 'ae', 'ea', 'ei', 'ou'];
const ENDS = ['n', 'r', 'l', 'm', 'th', 'sk', 'rn', 'ld', 'st', 'd', 'k', 'ss'];
const PLACE_TAIL = ['ford', 'holt', 'stead', 'wick', 'mere', 'combe', 'ridge', 'fell', 'burn', 'hollow', 'reach', 'dale'];
const TRADE_TAIL = ['company', 'caravan', 'wagons', 'road', 'traders', 'carriers'];

function syllable(rng) {
  return rng.pick(STARTS) + rng.pick(VOWELS);
}

export function personName(rng) {
  let s = syllable(rng);
  if (rng.chance(0.55)) s += syllable(rng);
  s += rng.pick(ENDS);
  return s[0].toUpperCase() + s.slice(1);
}

export function familyName(rng) {
  const s = syllable(rng) + rng.pick(ENDS);
  return s[0].toUpperCase() + s.slice(1) + rng.pick(['son', 'ler', 'wright', 'ward', 'man', 'field']);
}

export function placeName(rng) {
  const s = syllable(rng) + (rng.chance(0.4) ? syllable(rng) : '');
  return s[0].toUpperCase() + s.slice(1) + rng.pick(PLACE_TAIL);
}

export function caravanName(rng) {
  const s = syllable(rng) + rng.pick(ENDS);
  return `${s[0].toUpperCase()}${s.slice(1)} ${rng.pick(TRADE_TAIL)}`;
}
