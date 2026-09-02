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

/// The page and the crate are one program with one version, and the number the
/// top bar shows comes from the crate. A mismatch would have the two halves of
/// the same build claiming different things.
#[test]
fn the_package_and_the_crate_carry_the_same_version() {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../package.json");
    let raw = std::fs::read_to_string(manifest).expect("package.json");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("package.json parses");
    assert_eq!(json["version"].as_str(), Some(grow::VERSION));
}

/// An exported project names the build that wrote it, whatever the file it was
/// loaded from said.
#[test]
fn an_exported_project_is_stamped_with_the_build() {
    let json = State::new().to_json();
    assert!(json.contains(&format!("\"app\":\"{}\"", grow::VERSION)), "{json:.120}");
    let older = r#"{"version":3,"app":"0.0.1","seed":5}"#;
    let state = State::from_json(older).expect("older project loads");
    assert_eq!(state.app, grow::VERSION, "the stamp is rewritten, not carried over");
}

/// The map editor used to keep a draft in the project and lay it over the map
/// on Apply. It paints the map itself now, and a project written while the
/// draft existed still loads: an unknown field is ignored rather than refused.
#[test]
fn a_project_carrying_an_old_map_draft_still_loads() {
    let raw = r#"{"version":3,"app":"0.0.1","civ":{"seed":9,"mapDraft":{"cols":32,"rows":16,"paint":"1x8,0x504"}}}"#;
    let state = State::from_json(raw).expect("a project with a draft in it loads");
    assert_eq!(state.civ.seed, 9, "the rest of the settlement came through");
}
