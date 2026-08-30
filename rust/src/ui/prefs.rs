//! How this browser is set up to look, as opposed to what the project holds.
//!
//! Whether the menu is folded away and how large the text is are properties of
//! the window somebody is working in, not of the thing they are working on, so
//! they are kept in their own key and never travel in an exported project.
//!
//! Both are applied by setting something on the page and letting the
//! stylesheet do the rest: a class on the body for the fold, and a custom
//! property for the scale, which the root font size is multiplied by.

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use crate::ui::{document, window};

const KEY: &str = "grow.ui.v1";

/// The narrowest and widest the text is allowed to be scaled to. Below the
/// first the controls stop being usable, above the second the panel stops
/// fitting a phone.
pub const SCALE_MIN: f64 = 0.75;
pub const SCALE_MAX: f64 = 1.75;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// The side menu is folded away, leaving the map the whole window.
    pub collapsed: bool,
    /// What the root font size is multiplied by.
    pub scale: f64,
    /// Keep a copy of every sheet in the sprite store as the project saves, so
    /// art outlives the project it was drawn in.
    pub keep_sprites: bool,
    /// The menu sections folded shut, by their titles. A section is rebuilt
    /// whole whenever its panel is, so the fold has to be read back from
    /// somewhere the rebuild can reach or every change would spring it open.
    pub folded: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { collapsed: false, scale: 1.0, keep_sprites: true, folded: Vec::new() }
    }
}

impl Prefs {
    pub fn load() -> Prefs {
        let raw = window()
            .local_storage()
            .ok()
            .flatten()
            .and_then(|store| store.get_item(KEY).ok().flatten());
        let mut prefs: Prefs = raw
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        prefs.scale = prefs.scale.clamp(SCALE_MIN, SCALE_MAX);
        prefs
    }

    pub fn is_folded(&self, title: &str) -> bool {
        self.folded.iter().any(|t| t == title)
    }

    pub fn set_folded(&mut self, title: &str, folded: bool) {
        self.folded.retain(|t| t != title);
        if folded {
            self.folded.push(title.to_string());
        }
    }

    pub fn save(&self) {
        if let Ok(Some(store)) = window().local_storage() {
            if let Ok(raw) = serde_json::to_string(self) {
                let _ = store.set_item(KEY, &raw);
            }
        }
    }

    /// Puts the page in the state these describe. Called on load and again
    /// after either is changed.
    pub fn apply(&self) {
        if let Some(body) = document().body() {
            let list = body.class_list();
            let _ = if self.collapsed {
                list.add_1("menu-collapsed")
            } else {
                list.remove_1("menu-collapsed")
            };
        }
        if let Some(root) = document()
            .document_element()
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = root.style().set_property("--ui-scale", &format!("{}", self.scale));
        }
    }
}
