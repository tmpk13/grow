//! Menu search: that the index describes the page, and that typing the obvious
//! thing puts the obvious answer first.

use std::collections::HashMap;

use grow::find::{query_words, Entry, Index, Search, Terms, FLOOR};

fn index() -> Index {
    Index::builtin()
}

/// The label of the best match for a query, or None if nothing cleared the
/// floor.
fn best(index: &Index, query: &str) -> Option<String> {
    index
        .search(Search::new(query))
        .first()
        .map(|hit| index.entries[hit.idx].label.clone())
}

fn labels(index: &Index, query: &str) -> Vec<String> {
    index
        .search(Search::new(query))
        .iter()
        .map(|hit| index.entries[hit.idx].label.clone())
        .collect()
}

/// Where the best match for a query is, searching from a given mode and tab.
fn best_from(index: &Index, query: &str, mode: &str, tab: &str) -> (String, String, String) {
    let hit = index.search(Search::new(query).from(mode, tab));
    let e = &index.entries[hit.first().expect("something should match").idx];
    (e.mode.clone(), e.tab.clone(), e.label.clone())
}

#[test]
fn the_baked_index_parses() {
    // `Index::builtin` cannot report a bad file in a page, so it falls back to
    // an empty index. This is what would notice.
    let entries: Vec<Entry> =
        serde_json::from_str(grow::find::INDEX_JSON).expect("assets/menu-index.json");
    assert!(!entries.is_empty(), "the harvested index is empty");
}

#[test]
fn the_baked_meaning_table_is_for_the_baked_index() {
    // The table points at entries by position. One built against an older set
    // of menus points at the wrong things, and the app answers by dropping it
    // and hiding the switch, which is easy to miss. This is the loud version.
    let terms: Terms =
        serde_json::from_str(grow::find::TERMS_JSON).expect("assets/menu-terms.json");
    if terms.words.is_empty() {
        return; // No table built yet; the switch is simply not offered.
    }
    let index = index();
    assert_eq!(
        terms.stamp,
        index.stamp(),
        "the meaning table was built for other menus: run `bun run index:terms`"
    );
    assert!(index.has_terms(), "a table with the right stamp should have been kept");
    for list in terms.words.values() {
        for (idx, _) in list {
            assert!(
                (*idx as usize) < index.entries.len(),
                "the table points past the end of the index"
            );
        }
    }
}

#[test]
fn the_index_covers_every_mode_and_tab() {
    let index = index();
    assert!(
        index.entries.len() > 200,
        "only {} entries: the harvest cannot have visited every tab",
        index.entries.len()
    );

    let mut tabs: Vec<(String, String)> = index
        .entries
        .iter()
        .filter(|e| !e.mode.is_empty() && !e.tab.is_empty())
        .map(|e| (e.mode.clone(), e.tab.clone()))
        .collect();
    tabs.sort();
    tabs.dedup();
    assert_eq!(tabs.len(), 11, "every tab of every mode should be in the index: {tabs:?}");

    for mode in ["lab", "sprites", "settlement"] {
        assert!(
            index.entries.iter().any(|e| e.mode == mode),
            "nothing indexed for mode {mode}"
        );
    }
}

#[test]
fn every_entry_can_be_pointed_at() {
    let index = index();
    for e in &index.entries {
        assert!(!e.label.is_empty(), "an entry with no label: {e:?}");
        if e.kind == "tab" {
            // A tab is its own destination; there is no control to flash.
            assert!(e.anchor.is_empty(), "a tab should have no anchor: {e:?}");
            continue;
        }
        assert!(!e.anchor.is_empty(), "nothing to jump to: {e:?}");
        if e.kind == "chrome" {
            assert!(e.anchor.starts_with('#'), "chrome is addressed by id: {e:?}");
            assert!(e.mode.is_empty(), "chrome belongs to no mode: {e:?}");
        } else if e.kind == "view" {
            // The view menu belongs to a mode but sits beside the tabs, so it
            // is reachable from every one of them.
            assert!(!e.mode.is_empty(), "a view switch needs a mode: {e:?}");
            assert!(e.tab.is_empty(), "a view switch belongs to no tab: {e:?}");
        } else {
            assert!(!e.mode.is_empty(), "a panel control needs a mode: {e:?}");
            assert!(!e.tab.is_empty(), "a panel control needs a tab: {e:?}");
            assert!(
                e.anchor.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "an anchor has to survive being a selector: {e:?}"
            );
        }
    }
}

#[test]
fn an_anchor_is_unique_within_its_tab() {
    let index = index();
    let mut seen = std::collections::HashSet::new();
    for e in &index.entries {
        if e.anchor.is_empty() {
            continue;
        }
        let key = (e.mode.clone(), e.tab.clone(), e.kind.clone(), e.anchor.clone());
        assert!(seen.insert(key), "two controls answer to the same anchor: {e:?}");
    }
}

#[test]
fn typing_a_label_puts_it_first() {
    let index = index();
    for (query, want) in [
        ("seed", "Seed"),
        ("octaves", "Octaves"),
        ("roughness", "Roughness"),
        ("water level", "Water level"),
        ("wilderness warmup", "Wilderness warmup (s)"),
    ] {
        assert_eq!(best(&index, query).as_deref(), Some(want), "searching for {query}");
    }
}

#[test]
fn half_a_label_is_enough() {
    let index = index();
    assert_eq!(best(&index, "wilder").as_deref(), Some("Wilderness warmup (s)"));
    assert_eq!(best(&index, "rough").as_deref(), Some("Roughness"));
}

#[test]
fn every_word_typed_has_to_land() {
    let index = index();
    // Both words are in the index, but never on the same control.
    let hits = index.search(Search::new("octaves treasury"));
    assert!(hits.is_empty(), "an unmatchable pair should find nothing, got {hits:?}");
}

#[test]
fn a_second_word_narrows_the_list() {
    let index = index();
    let one = index.search(Search { query: "cell", limit: 30, ..Search::default() }).len();
    let two = index.search(Search { query: "cell depth", limit: 30, ..Search::default() }).len();
    assert!(two < one, "cell -> {one} hits, cell depth -> {two}");
    assert_eq!(best(&index, "cell depth").as_deref(), Some("Cell depth (px)"));
}

#[test]
fn a_match_on_the_screen_you_are_looking_at_comes_first() {
    let index = index();
    // Seed is a setting of the plant world and of the settlement map both.
    assert_eq!(
        best_from(&index, "seed", "lab", "world"),
        ("lab".into(), "world".into(), "Seed".into())
    );
    assert_eq!(
        best_from(&index, "seed", "settlement", "land"),
        ("settlement".into(), "land".into(), "Seed".into())
    );
    // And the brush is a control of two different modes.
    assert_eq!(best_from(&index, "brush color", "sprites", "draw").0, "sprites");
    assert_eq!(best_from(&index, "brush color", "lab", "materials").0, "lab");
}

#[test]
fn the_tab_you_are_on_beats_another_tab_of_the_same_mode() {
    let index = index();
    // Name is a box's, a species' and a sheet's.
    assert_eq!(best_from(&index, "name", "lab", "species").1, "species");
    assert_eq!(best_from(&index, "name", "lab", "materials").1, "materials");
}

#[test]
fn being_on_a_tab_cannot_outrank_a_better_match_elsewhere() {
    let index = index();
    // Sitting on the tab that owns "Octaves" must not make its neighbours beat
    // an exact hit somewhere else.
    let (_, _, label) = best_from(&index, "roughness", "settlement", "land");
    assert_eq!(label, "Roughness");
    let (_, _, label) = best_from(&index, "meander", "lab", "materials");
    assert_eq!(label, "Meander");
}

#[test]
fn where_you_are_cannot_drag_in_what_did_not_match() {
    let index = index();
    let plain = index.search(Search::new("qqzzxx"));
    let local = index.search(Search::new("qqzzxx").from("settlement", "land"));
    assert!(plain.is_empty() && local.is_empty());
}

#[test]
fn an_empty_box_starts_where_you_are() {
    let index = index();
    let hits = index.search(Search::new("").from("settlement", "economy"));
    let first = &index.entries[hits[0].idx];
    assert_eq!((first.mode.as_str(), first.tab.as_str()), ("settlement", "economy"));
}

#[test]
fn nonsense_finds_nothing() {
    let index = index();
    assert!(index.search(Search::new("qqzzxx")).is_empty());
    assert!(index.search(Search::new("zzzzzzzzzz")).is_empty());
}

#[test]
fn an_empty_query_lists_the_menus() {
    let index = index();
    let hits = index.search(Search::new(""));
    assert_eq!(hits.len(), 12, "an empty box should be an outline, not a blank");
    assert!(hits.iter().all(|h| h.score == 0.0));
    assert_eq!(hits[0].idx, 0, "in index order, so the list does not jump about");
}

#[test]
fn the_chrome_is_reachable_from_anywhere() {
    let index = index();
    for (query, want) in [("undo", "Undo"), ("fullscreen", "Fullscreen"), ("export", "Export")] {
        let hit = index.search(Search { query, limit: 8, ..Search::default() });
        let found = hit.iter().any(|h| index.entries[h.idx].label == want);
        assert!(found, "searching {query} should offer {want}, got {:?}", labels(&index, query));
    }
}

#[test]
fn the_view_menu_is_reachable_from_any_tab_of_its_mode() {
    let index = index();
    let walls = index
        .entries
        .iter()
        .find(|e| e.kind == "view" && e.label == "Walls and gates")
        .expect("the wall labels have a switch of their own");
    assert_eq!(walls.mode, "settlement");
    assert!(walls.tab.is_empty());
    // Reachable while standing on a tab that knows nothing about it.
    let (_, _, label) = best_from(&index, "walls and gates", "settlement", "economy");
    assert_eq!(label, "Walls and gates");
}

#[test]
fn a_path_reads_as_a_place() {
    let index = index();
    let seed = index
        .entries
        .iter()
        .find(|e| e.label == "Seed" && e.mode == "settlement")
        .expect("the settlement has a seed");
    assert_eq!(seed.path(), "Settlement / Land / Map");
}

#[test]
fn punctuation_in_a_query_is_only_a_separator() {
    assert_eq!(query_words("cell-depth (px)"), vec!["cell", "depth", "px"]);
    let index = index();
    assert_eq!(best(&index, "cell-depth").as_deref(), Some("Cell depth (px)"));
}

// ---- meaning matching ----------------------------------------------------

fn tiny() -> Index {
    Index::new(vec![
        Entry {
            mode: "settlement".into(),
            mode_label: "Settlement".into(),
            tab: "land".into(),
            tab_label: "Land".into(),
            label: "Water level".into(),
            anchor: "water-level".into(),
            kind: "field".into(),
            ..Entry::default()
        },
        Entry {
            mode: "settlement".into(),
            mode_label: "Settlement".into(),
            tab: "economy".into(),
            tab_label: "Economy".into(),
            label: "Pays wages".into(),
            anchor: "pays-wages".into(),
            kind: "field".into(),
            ..Entry::default()
        },
    ])
}

fn terms_for(index: &Index, pairs: &[(&str, u32, u8)]) -> Terms {
    let mut words: HashMap<String, Vec<(u32, u8)>> = HashMap::new();
    for (word, idx, score) in pairs {
        words.entry((*word).into()).or_default().push((*idx, *score));
    }
    Terms { stamp: index.stamp(), words }
}

#[test]
fn a_meaning_table_built_for_another_index_is_refused() {
    let mut index = tiny();
    let wrong = Terms { stamp: "not this one".into(), ..Terms::default() };
    assert!(!index.set_terms(wrong));
    assert!(!index.has_terms());
}

#[test]
fn meaning_finds_what_the_letters_cannot() {
    let mut index = tiny();
    let terms = terms_for(&index, &[("salary", 1, 230)]);
    assert!(index.set_terms(terms));

    assert!(index.search(Search::new("salary")).is_empty(), "no letters of salary are in the index");

    let hits = index.search(Search { query: "salary", by_meaning: true, ..Search::default() });
    assert_eq!(hits.len(), 1);
    assert_eq!(index.entries[hits[0].idx].label, "Pays wages");
    assert!(hits[0].by_meaning, "the row should say the letters did not find it");
    assert!(hits[0].score >= FLOOR);
}

#[test]
fn meaning_never_pushes_an_exact_label_down() {
    let mut index = tiny();
    // A table that likes the wrong entry as much as it can.
    let terms = terms_for(&index, &[("water", 1, 255)]);
    assert!(index.set_terms(terms));

    let hits = index.search(Search { query: "water level", by_meaning: true, ..Search::default() });
    assert_eq!(index.entries[hits[0].idx].label, "Water level");
    assert!(!hits[0].by_meaning);
}

#[test]
fn the_switch_does_nothing_without_a_table() {
    let index = tiny();
    assert!(!index.has_terms());
    assert_eq!(index.search(Search { query: "salary", by_meaning: true, ..Search::default() }).len(), 0);
    assert_eq!(
        index.search(Search { query: "water", by_meaning: true, ..Search::default() }).len(),
        index.search(Search::new("water")).len()
    );
}

#[test]
fn a_stamp_changes_with_the_menus() {
    let a = tiny();
    let mut entries = a.entries.clone();
    entries.push(Entry { label: "New thing".into(), anchor: "new-thing".into(), ..Entry::default() });
    let b = Index::new(entries);
    assert_ne!(a.stamp(), b.stamp());
}
