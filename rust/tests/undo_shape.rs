//! The `app_*` panel helpers record an undo step before they run. A step is a
//! snapshot of the project, so a control wired through one of them has to
//! change the project: a closure that only writes to `app.ui` pushes a step
//! that restores nothing, and undo then appears to skip a press.
//!
//! Nothing in the type system says so - the helpers hand out `&mut App`, and
//! `app.ui` is right there - so this reads the panels and says so instead.

use std::fs;
use std::path::{Path, PathBuf};

/// The helpers that record a step. Anything called through one of these is
/// claiming to change the project.
const HELPERS: [&str; 8] = [
    "app_num",
    "app_range",
    "app_bool",
    "app_text",
    "app_select",
    "app_color",
    "app_button",
    "app_danger_button",
];

fn sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("reading src") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The text between the parentheses of the call starting at `open`, which is
/// the index of the `(`. None if the call is not closed in this file.
fn call_body(src: &str, open: usize) -> Option<&str> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[open + 1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Every call to one of the helpers, as (file, line, the text inside the call).
fn helper_calls() -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    for path in sources() {
        let src = fs::read_to_string(&path).expect("reading a source file");
        // The helpers are defined in ui/mod.rs; their own definitions are not
        // call sites.
        let defining = path.ends_with("ui/mod.rs");
        for helper in HELPERS {
            let needle = format!("{helper}(");
            let mut from = 0usize;
            while let Some(rel) = src[from..].find(&needle) {
                let at = from + rel;
                from = at + needle.len();
                // Only a call: `pub fn app_num(` is a definition, and
                // `crate::ui::app_num(` is still a call.
                let before = src[..at].trim_end();
                if before.ends_with("fn") {
                    continue;
                }
                if defining && before.ends_with("pub") {
                    continue;
                }
                let open = at + needle.len() - 1;
                if let Some(body) = call_body(&src, open) {
                    let line = src[..at].bytes().filter(|b| *b == b'\n').count() + 1;
                    let name = path
                        .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")))
                        .unwrap_or(&path)
                        .display()
                        .to_string();
                    out.push((name, line, body.to_string()));
                }
            }
        }
    }
    out
}

/// Helpers that take the app and change the project through it. A closure that
/// calls one of these is changing the project even though `app.state` is
/// nowhere in its own text.
///
/// The list is deliberately short and deliberately explicit: adding to it is a
/// statement that the named helper writes to the project, and a new indirection
/// that nobody has vouched for makes this check fire rather than pass quietly.
const PROJECT_HELPERS: [&str; 2] = ["with_sheet(", "preset("];

fn touches_project(body: &str) -> bool {
    body.contains("app.state") || PROJECT_HELPERS.iter().any(|h| body.contains(h))
}

/// Whether the text assigns to something under `app.ui`. `==` is a comparison
/// and `>=` and friends cannot follow a field path here, so one character of
/// lookahead is enough.
fn writes_to_ui(body: &str) -> bool {
    let mut from = 0usize;
    while let Some(rel) = body[from..].find("app.ui.") {
        let at = from + rel;
        from = at + "app.ui.".len();
        let rest = &body[from..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let after = rest[end..].trim_start();
        let assigns = after.starts_with('=') && !after.starts_with("==");
        // A method that changes it in place counts too.
        let mutates = after.starts_with(".push(")
            || after.starts_with(".clear(")
            || after.starts_with(".remove(")
            || after.starts_with(".insert(");
        if assigns || mutates {
            return true;
        }
    }
    false
}

#[test]
fn the_scan_finds_the_panels() {
    let calls = helper_calls();
    assert!(
        calls.len() > 60,
        "only {} helper calls found: the scan has lost track of the panels",
        calls.len()
    );
    let mut files: Vec<&str> = calls.iter().map(|(f, _, _)| f.as_str()).collect();
    files.sort();
    files.dedup();
    assert!(files.len() > 5, "the helpers should be used by more than {} files", files.len());
}

#[test]
fn every_named_project_helper_still_exists() {
    // A helper that has been renamed away would silently widen the check into
    // accepting anything, so the names are checked against the source.
    let all: String = sources()
        .iter()
        .map(|p| fs::read_to_string(p).expect("reading a source file"))
        .collect();
    for helper in PROJECT_HELPERS {
        let name = helper.trim_end_matches('(');
        assert!(
            all.contains(&format!("fn {name}")),
            "`{name}` is vouched for as changing the project but no longer exists"
        );
    }
}

#[test]
fn no_undoable_control_only_changes_the_ui() {
    let mut bad = Vec::new();
    for (file, line, body) in helper_calls() {
        if writes_to_ui(&body) && !touches_project(&body) {
            bad.push(format!("{file}:{line}"));
        }
    }
    assert!(
        bad.is_empty(),
        "these controls record an undo step and then change only `app.ui`, which a \
         snapshot of the project cannot put back. Wire them with a plain `on(...)` \
         listener instead, or make them change the project:\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn the_detector_knows_the_shape_it_is_looking_for() {
    // The bug: a step is recorded and then only the UI moves.
    let bad = r#"h, "Onion", app.ui.onion, None, |app, v| { app.ui.onion = v; }"#;
    assert!(writes_to_ui(bad) && !touches_project(bad));

    // Fine: the project changes, and the selection follows it.
    let good = r#"h, "Add layer", |app| {
        let at = app.ui.sheet_layer;
        with_sheet(app, |s| s.add_layer(at, "Layer"));
        app.ui.sheet_layer = at;
    }"#;
    assert!(writes_to_ui(good) && touches_project(good));

    // Fine: reads the UI to find what to change, changes the project.
    let reads = r#"h, "Seed", v, opts, None, |app, v| { app.state.civ.seed = v as u32; }"#;
    assert!(!writes_to_ui(reads));

    // A comparison is not an assignment.
    let compares = r#"|app, v| { if app.ui.tool == Tool::Pick { app.state.x = v; } }"#;
    assert!(!writes_to_ui(compares));
}
