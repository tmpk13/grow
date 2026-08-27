//! grow: a pixel art plant lab and a settlement that has to gather what it
//! builds, compiled to WebAssembly.
//!
//! The crate is split so that everything below `app` and `ui` is plain Rust
//! with no browser dependency: the simulation runs headless in the smoke test
//! binaries, and the same code drives the page in a browser.

pub mod art;
pub mod civ;
pub mod plant;
pub mod rng;
pub mod sampler;
pub mod shading;
pub mod sim;
pub mod species;
pub mod state;
pub mod util;
pub mod world;

#[cfg(target_arch = "wasm32")]
pub mod app;
#[cfg(target_arch = "wasm32")]
pub mod render;
#[cfg(target_arch = "wasm32")]
pub mod ui;
