//! Sheets kept outside the project.
//!
//! A project is one thing somebody is working on and can be replaced wholesale
//! by New, Import or a reset. Art outlives that: a settler drawn once is worth
//! keeping across every project it might turn up in, and losing it to a button
//! meant to clear a stuck page is not a trade anybody would make.
//!
//! So sheets are also written to a store of their own, under their own key,
//! which the reset deliberately puts back afterwards. Nothing reads it except
//! the panel: the project still holds the sheets that are being drawn, and this
//! is only where copies of them wait.

use crate::art::{ArtLibrary, Sheet};
use crate::ui::window;

pub const KEY: &str = "grow.sprites.v1";

fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

/// Everything the store is holding. A store that will not parse reads as empty
/// rather than as a failure: it is a convenience, and refusing to show the
/// panel because of it would help nobody.
pub fn load() -> ArtLibrary {
    let raw = storage().and_then(|s| s.get_item(KEY).ok().flatten());
    let mut lib: ArtLibrary = raw
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(ArtLibrary { sheets: Vec::new() });
    lib.fit();
    lib
}

/// The raw text, for the reset to hold on to while it clears everything else.
pub fn raw() -> Option<String> {
    storage().and_then(|s| s.get_item(KEY).ok().flatten())
}

pub fn put_raw(raw: &str) {
    if let Some(store) = storage() {
        let _ = store.set_item(KEY, raw);
    }
}

fn save(lib: &ArtLibrary) -> bool {
    match (storage(), serde_json::to_string(lib)) {
        (Some(store), Ok(raw)) => store.set_item(KEY, &raw).is_ok(),
        _ => false,
    }
}

/// Copies the project's sheets in, replacing any already held under the same
/// id. Sheets in the store that the project no longer has are left alone: this
/// is a place things are kept, not a mirror of what is open.
pub fn keep(project: &ArtLibrary) -> bool {
    let mut lib = load();
    for sheet in &project.sheets {
        match lib.index_of(&sheet.id) {
            Some(at) => lib.sheets[at] = sheet.clone(),
            None => lib.sheets.push(sheet.clone()),
        }
    }
    save(&lib)
}

pub fn remove(id: &str) -> bool {
    let mut lib = load();
    match lib.index_of(id) {
        Some(at) => {
            lib.sheets.remove(at);
            save(&lib)
        }
        None => false,
    }
}

pub fn find(id: &str) -> Option<Sheet> {
    load().find(id).cloned()
}

/// What the store costs, as the characters it takes to write it down.
pub fn bytes() -> usize {
    raw().map(|r| r.len()).unwrap_or(0)
}
