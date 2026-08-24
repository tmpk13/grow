//! A project file written before this rewrite still has to load, so the JSON
//! shape is pinned here against a file exported by the previous version.

use grow::sampler::MaterialMode;
use grow::species::SizeClass;
use grow::state::State;

const LEGACY: &str = include_str!("fixtures/js-project.json");

#[test]
fn loads_a_project_exported_by_the_previous_version() {
    let state = State::from_json(LEGACY).expect("legacy project loads");
    assert_eq!(state.seed, 4242);
    assert_eq!(state.version, grow::state::STATE_VERSION);
    assert_eq!(state.materials.mode, MaterialMode::Multi);
    assert_eq!(state.materials.samplers.len(), 14);
    assert_eq!(state.materials.samplers[0].px[0], 0xff11_2233);
    assert_eq!(state.species.len(), 5);
    assert_eq!(state.species[3].id, "sp-oak");
    assert_eq!(state.species[3].size_class, SizeClass::Tree);
    assert!((state.species[3].form.branch_chance - 0.31).abs() < 1e-9);
    assert!((state.civ.terrain.wildness - 3.7).abs() < 1e-9);
    assert_eq!(state.civ.start.supplies[grow::civ::resources::Res::Wood as usize], 99.0);
    assert_eq!(state.class_limits.tree.max_radius_cells, 5);
    assert_eq!(state.world.cols, 64);
    assert_eq!(state.civ.world.cols, 88);
}

#[test]
fn a_project_survives_a_save_and_load_round_trip() {
    let mut state = State::new();
    state.seed = 7;
    state.species[0].name = "Round trip".into();
    state.civ.view.labels = true;
    state.materials.samplers[2].px[5] = 0x8899_aabb;
    let json = state.to_json();
    let back = State::from_json(&json).expect("round trip");
    assert_eq!(back.seed, 7);
    assert_eq!(back.species[0].name, "Round trip");
    assert!(back.civ.view.labels);
    assert_eq!(back.materials.samplers[2].px[5], 0x8899_aabb);
}

/// A project saved before the settlement existed keeps its species and drops
/// only the world config the model outgrew.
#[test]
fn an_older_project_is_upgraded_rather_than_rejected() {
    let raw = r#"{"version":1,"seed":11,"world":{"cols":9999},"species":[{"id":"sp-x","name":"X"}]}"#;
    let state = State::from_json(raw).expect("old project loads");
    assert_eq!(state.seed, 11);
    assert_eq!(state.world.cols, 64, "a version 1 world config is discarded");
    assert_eq!(state.species.len(), 1);
    assert_eq!(state.species[0].name, "X");
    assert_eq!(state.species[0].size_class, SizeClass::Shrub, "missing fields fall back");
    assert_eq!(state.materials.samplers.len(), 14, "missing materials are rebuilt");
}
