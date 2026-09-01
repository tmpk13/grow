//! Procedural names. People, their settlement and the caravans that visit are
//! all named from the same syllable tables so a run reads as one place.

use crate::rng::Rng;

const STARTS: [&str; 18] = [
    "b", "br", "d", "f", "g", "gr", "h", "k", "l", "m", "n", "r", "s", "st", "t", "th", "v", "w",
];
const VOWELS: [&str; 9] = ["a", "e", "i", "o", "u", "ae", "ea", "ei", "ou"];
const ENDS: [&str; 12] = ["n", "r", "l", "m", "th", "sk", "rn", "ld", "st", "d", "k", "ss"];
const PLACE_TAIL: [&str; 12] = [
    "ford", "holt", "stead", "wick", "mere", "combe", "ridge", "fell", "burn", "hollow", "reach",
    "dale",
];
const TRADE_TAIL: [&str; 6] = ["company", "caravan", "wagons", "road", "traders", "carriers"];
const FAMILY_TAIL: [&str; 6] = ["son", "ler", "wright", "ward", "man", "field"];

fn syllable(rng: &mut Rng) -> String {
    format!("{}{}", rng.pick(&STARTS), rng.pick(&VOWELS))
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn person_name(rng: &mut Rng) -> String {
    let mut s = syllable(rng);
    if rng.chance(0.55) {
        s.push_str(&syllable(rng));
    }
    s.push_str(rng.pick(&ENDS));
    capitalize(&s)
}

pub fn family_name(rng: &mut Rng) -> String {
    let s = format!("{}{}", syllable(rng), rng.pick(&ENDS));
    format!("{}{}", capitalize(&s), rng.pick(&FAMILY_TAIL))
}

pub fn place_name(rng: &mut Rng) -> String {
    let mut s = syllable(rng);
    if rng.chance(0.4) {
        s.push_str(&syllable(rng));
    }
    format!("{}{}", capitalize(&s), rng.pick(&PLACE_TAIL))
}

pub fn caravan_name(rng: &mut Rng) -> String {
    let s = format!("{}{}", syllable(rng), rng.pick(&ENDS));
    format!("{} {}", capitalize(&s), rng.pick(&TRADE_TAIL))
}

const RIVER_TAIL: [&str; 8] = [
    "water", "brook", "run", "flow", "beck", "rill", "wash", "current",
];
const INN_HEAD: [&str; 8] = [
    "The Old", "The Long", "The Broken", "The Gilded", "The Quiet", "The Crooked",
    "The Silver", "The Wayside",
];
const INN_TAIL: [&str; 10] = [
    "Oar", "Anvil", "Lantern", "Barrel", "Stag", "Ford", "Millstone", "Hearth", "Bell", "Ferry",
];
const BOAT_TAIL: [&str; 8] = [
    "Maid", "Otter", "Heron", "Barge", "Skiff", "Reed", "Drift", "Willow",
];

pub fn river_name(rng: &mut Rng) -> String {
    let mut s = syllable(rng);
    if rng.chance(0.45) {
        s.push_str(&syllable(rng));
    }
    format!("{}{}", capitalize(&s), rng.pick(&RIVER_TAIL))
}

/// Inns are named after a thing on a sign, not after their owner, so a town
/// with three of them reads as three places rather than three people.
pub fn inn_name(rng: &mut Rng) -> String {
    let sign = if rng.chance(0.5) {
        *rng.pick(&INN_TAIL)
    } else {
        *rng.pick(&BOAT_TAIL)
    };
    format!("{} {}", rng.pick(&INN_HEAD), sign)
}

pub fn boat_name(rng: &mut Rng) -> String {
    let s = format!("{}{}", syllable(rng), rng.pick(&ENDS));
    format!("{} {}", capitalize(&s), rng.pick(&BOAT_TAIL))
}
