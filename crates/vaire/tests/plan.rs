//! `emit::plan` semantics: dry-run leaves the file untouched, and the plan
//! matches what `emit` would splice.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]

use std::fs;

const SOURCE: &str = "[requirement#SWR-0001]\n[status=draft]\n--\nThe system shall beep.\n--\n";

fn temp_source(dir: &tempfile::TempDir) -> String {
    let path = dir.path().join("vaire-plan.adoc");
    let path = path.to_str().unwrap().to_owned();
    fs::write(&path, SOURCE).unwrap();
    path
}

fn records_json(path: &str, status: &str) -> String {
    let source = fs::read_to_string(path).unwrap();
    let mut records = vaire::extract::extract(path, &source).unwrap();
    for line in &mut records[0].attr_lines {
        for item in &mut line.items {
            if let vaire::Item::Kv { name, value } = item
                && name == "status"
            {
                *value = status.to_owned();
            }
        }
    }
    serde_json::to_string(&records).unwrap()
}

#[test]
fn plan_reports_edit_without_touching_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_source(&dir);
    let before = fs::read_to_string(&path).unwrap();

    let edits = vaire::emit::plan(&records_json(&path, "active"), &path).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].id, "SWR-0001");
    assert_eq!(edits[0].old, "[status=draft]");
    assert_eq!(edits[0].new, "[status=active]");

    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn plan_is_empty_when_nothing_differs() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_source(&dir);
    let edits = vaire::emit::plan(&records_json(&path, "draft"), &path).unwrap();
    assert!(edits.is_empty());
}

#[test]
fn emit_applies_the_planned_edit() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_source(&dir);
    vaire::emit::emit(&records_json(&path, "active"), &path).unwrap();
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("[status=active]"));
    assert!(after.contains("The system shall beep."));
}
