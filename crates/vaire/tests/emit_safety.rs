//! `emit` safety: path identity, record accounting, and stale-source
//! protection. Every rejection leaves the target byte-identical.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]

use std::fs;
use std::path::Path;

const ONE: &str = "[requirement#SWR-0001]\n[status=draft]\n--\nThe system shall beep.\n--\n";
const TWO: &str = "[requirement#SWR-0001]\n[status=draft]\n--\nThe system shall beep.\n--\n\n[requirement#SWR-0002]\n[status=draft]\n--\nThe system shall log.\n--\n";
/// `TWO` with only SWR-0001's status line edited.
const TWO_EDITED: &str = "[requirement#SWR-0001]\n[status=active]\n--\nThe system shall beep.\n--\n\n[requirement#SWR-0002]\n[status=draft]\n--\nThe system shall log.\n--\n";

fn write_source(dir: &Path, name: &str, source: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, source).unwrap();
    path.to_str().unwrap().to_owned()
}

fn extract_records(path: &str) -> Vec<vaire::Record> {
    let source = fs::read_to_string(path).unwrap();
    vaire::extract::extract(path, &source).unwrap()
}

fn json(records: &[vaire::Record]) -> String {
    serde_json::to_string(records).unwrap()
}

/// Records as they survive the JSON round trip, like a real caller's input.
fn parsed(path: &str) -> Vec<vaire::Record> {
    serde_json::from_str(&json(&extract_records(path))).unwrap()
}

fn set_status(record: &mut vaire::Record, status: &str) {
    for line in &mut record.attr_lines {
        for item in &mut line.items {
            if let vaire::Item::Kv { name, value } = item
                && name == "status"
            {
                *value = status.to_owned();
            }
        }
    }
}

fn status_json(path: &str, status: &str) -> String {
    let mut records = extract_records(path);
    for record in &mut records {
        set_status(record, status);
    }
    json(&records)
}

fn assert_unchanged(path: &str, before: &str) {
    assert_eq!(
        fs::read(path).unwrap(),
        before.as_bytes(),
        "{path} was written"
    );
}

#[test]
fn emit_rejects_records_from_a_different_file() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_source(dir.path(), "a.adoc", ONE);
    let b = write_source(dir.path(), "b.adoc", ONE);
    let before = fs::read_to_string(&b).unwrap();
    let json = json(&extract_records(&a));

    let error = vaire::emit::plan(&json, &b).unwrap_err();
    let message = error.to_string();
    assert!(message.contains(&a), "names the record's file: {message}");
    assert!(message.contains(&b), "names the target: {message}");
    assert!(vaire::emit::emit(&json, &b).is_err());
    assert_unchanged(&b, &before);
}

#[test]
fn emit_rejects_mixed_file_records() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_source(dir.path(), "a.adoc", ONE);
    let b = write_source(dir.path(), "b.adoc", ONE);
    let mut mixed = extract_records(&a);
    mixed.extend(extract_records(&b));
    let json = json(&mixed);

    let before_a = fs::read_to_string(&a).unwrap();
    let before_b = fs::read_to_string(&b).unwrap();
    assert!(vaire::emit::emit(&json, &a).is_err());
    assert!(vaire::emit::emit(&json, &b).is_err());
    assert_unchanged(&a, &before_a);
    assert_unchanged(&b, &before_b);
}

#[test]
fn absolute_dotted_and_relative_aliases_are_one_file() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let name = dir.path().file_name().unwrap().to_str().unwrap().to_owned();
    let path = dir.path().join("alias.adoc");
    fs::write(&path, ONE).unwrap();
    let absolute = fs::canonicalize(&path)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // A redundant `.` component, a `..` step, and a relative `./` path all
    // resolve to the same identity.
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    let dotted = format!("{}/./alias.adoc", dir.path().display());
    let up_down = format!("{}/sub/../alias.adoc", dir.path().display());
    let relative = format!("./{name}/alias.adoc");
    for alias in [&dotted, &up_down, &relative] {
        assert!(
            vaire::emit::plan(&json(&extract_records(&absolute)), alias).is_ok(),
            "alias {alias} rejected"
        );
    }

    // An emit through the relative alias lands on the same bytes.
    vaire::emit::emit(&status_json(&absolute, "active"), &relative).unwrap();
    assert_eq!(
        fs::read(&absolute).unwrap(),
        ONE.replace("[status=draft]", "[status=active]").as_bytes()
    );
}

#[test]
#[cfg(unix)]
fn symlink_alias_writes_the_real_file() {
    // Documented policy: identity canonicalizes, so symlinks resolve — and
    // the atomic rename lands on the canonical target, keeping the link.
    let dir = tempfile::tempdir().unwrap();
    let real = write_source(dir.path(), "real.adoc", ONE);
    let link_path = dir.path().join("link.adoc");
    std::os::unix::fs::symlink(&real, &link_path).unwrap();
    let link = link_path.to_str().unwrap().to_owned();

    // Extracted through the link, applied at the real path.
    vaire::emit::emit(&status_json(&link, "active"), &real).unwrap();
    assert!(
        fs::read_to_string(&real)
            .unwrap()
            .contains("[status=active]")
    );

    // Extracted at the real path, applied through the link.
    vaire::emit::emit(&status_json(&real, "draft"), &link).unwrap();
    assert!(
        fs::read_to_string(&real)
            .unwrap()
            .contains("[status=draft]")
    );
    assert!(
        fs::symlink_metadata(&link_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn emit_rejects_an_unknown_record_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "unknown-id.adoc", ONE);
    let before = fs::read_to_string(&path).unwrap();
    let mut records = parsed(&path);
    records[0].id = "SWR-4040".to_owned();
    assert!(vaire::emit::emit(&json(&records), &path).is_err());
    assert_unchanged(&path, &before);
}

#[test]
fn emit_rejects_a_mutated_id_item() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "mutated-id.adoc", ONE);
    let before = fs::read_to_string(&path).unwrap();
    let mut records = parsed(&path);
    for item in &mut records[0].attr_lines[0].items {
        if let vaire::Item::Id(id) = item {
            *id = "SWR-9999".to_owned();
        }
    }
    assert!(vaire::emit::emit(&json(&records), &path).is_err());
    assert_unchanged(&path, &before);
}

#[test]
fn emit_rejects_a_mutated_delimiter() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "mutated-delim.adoc", ONE);
    let before = fs::read_to_string(&path).unwrap();
    let mut records = parsed(&path);
    records[0].delim = "====".to_owned();
    assert!(vaire::emit::emit(&json(&records), &path).is_err());
    assert_unchanged(&path, &before);
}

#[test]
fn emit_rejects_a_mutated_body() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "mutated-body.adoc", ONE);
    let before = fs::read_to_string(&path).unwrap();
    let mut records = parsed(&path);
    records[0].body_raw = "The system shall not beep.\n".to_owned();
    assert!(vaire::emit::emit(&json(&records), &path).is_err());
    assert_unchanged(&path, &before);
}

#[test]
fn emit_rejects_a_mutated_span() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "mutated-span.adoc", ONE);
    let before = fs::read_to_string(&path).unwrap();
    let mut records = parsed(&path);
    records[0].span.end += 1;
    assert!(vaire::emit::emit(&json(&records), &path).is_err());
    assert_unchanged(&path, &before);
}

#[test]
fn emit_rejects_a_changed_attribute_line_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "changed-count.adoc", ONE);
    let before = fs::read_to_string(&path).unwrap();
    let mut records = parsed(&path);
    let extra = records[0].attr_lines[0].clone();
    records[0].attr_lines.push(extra);
    assert!(vaire::emit::emit(&json(&records), &path).is_err());
    assert_unchanged(&path, &before);
}

#[test]
fn emit_rejects_duplicate_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "duplicate.adoc", ONE);
    let before = fs::read_to_string(&path).unwrap();
    let mut records = parsed(&path);
    let mut copy = records[0].clone();
    set_status(&mut copy, "active");
    records.push(copy);
    assert!(vaire::emit::emit(&json(&records), &path).is_err());
    assert_unchanged(&path, &before);
}

#[test]
fn emit_rejects_when_an_edited_line_changed_since_extraction() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "stale-line.adoc", ONE);
    let json = status_json(&path, "active"); // extracted while the line said draft
    let stale = fs::read_to_string(&path)
        .unwrap()
        .replace("[status=draft]", "[status=paused]");
    fs::write(&path, &stale).unwrap(); // someone edited the line after extraction

    let error = vaire::emit::emit(&json, &path).unwrap_err();
    assert!(error.to_string().contains("re-extract"), "{error}");
    assert_eq!(
        fs::read(&path).unwrap(),
        stale.as_bytes(),
        "the manual edit was overwritten"
    );
}

#[test]
fn emit_rejects_when_bytes_were_inserted_before_the_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "stale-span.adoc", ONE);
    let json = status_json(&path, "active");
    let shifted = format!("// a new preamble line\n{ONE}");
    fs::write(&path, &shifted).unwrap();

    assert!(vaire::emit::emit(&json, &path).is_err());
    assert_eq!(fs::read(&path).unwrap(), shifted.as_bytes());
}

#[test]
fn emit_rejects_when_the_body_changed_since_extraction() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "stale-body.adoc", ONE);
    let json = status_json(&path, "active");
    // Same length, so the span still matches and the body check fires.
    let edited = ONE.replace("beep", "hum!");
    fs::write(&path, &edited).unwrap();

    assert!(vaire::emit::emit(&json, &path).is_err());
    assert_eq!(fs::read(&path).unwrap(), edited.as_bytes());
}

#[test]
fn emit_edits_only_the_intended_attribute_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_source(dir.path(), "surgical.adoc", TWO);
    let mut records = parsed(&path);
    let target = records.iter().position(|r| r.id == "SWR-0001").unwrap();
    set_status(&mut records[target], "active");

    vaire::emit::emit(&json(&records), &path).unwrap();
    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(after, TWO_EDITED);
    assert!(after.starts_with("[requirement#SWR-0001]\n"));
    assert!(after.ends_with(
        "\n--\n\n[requirement#SWR-0002]\n[status=draft]\n--\nThe system shall log.\n--\n"
    ));
}
