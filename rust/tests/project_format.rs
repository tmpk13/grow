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

/// A hand drawn map travels with the project, as runs rather than as a list of
/// eight thousand numbers, and a project written before there was one still
/// loads.
#[test]
fn a_map_draft_survives_a_round_trip_and_an_old_project_has_none() {
    use grow::civ::map_draft::Brush;

    let mut state = State::new();
    let draft = &mut state.civ.map_draft;
    draft.cols = 32;
    draft.rows = 16;
    draft.ensure();
    for c in 0..8 {
        draft.set(c, 3, Brush::Water);
    }
    draft.set(0, 0, Brush::Sky);
    draft.set(31, 15, Brush::Cliff);

    let raw = serde_json::to_string(&state).expect("write");
    // Runs, not numbers: the whole grid is a short string.
    assert!(raw.len() < 400_000, "the project got very large: {} bytes", raw.len());
    let back: State = serde_json::from_str(&raw).expect("read");
    assert_eq!(back.civ.map_draft.cols, 32);
    assert_eq!(back.civ.map_draft.rows, 16);
    assert_eq!(back.civ.map_draft.paint, state.civ.map_draft.paint);
    assert_eq!(back.civ.map_draft.at(4, 3), Brush::Water);
    assert_eq!(back.civ.map_draft.at(0, 0), Brush::Sky);
    assert_eq!(back.civ.map_draft.at(31, 15), Brush::Cliff);
    assert_eq!(back.civ.map_draft.at(4, 4), Brush::Clear);

    let old: State = serde_json::from_str("{}").expect("an empty project is a default one");
    assert!(old.civ.map_draft.nothing_painted(), "a project with no drawing came back with one");
}
