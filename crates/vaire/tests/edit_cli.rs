//! `edit`: surgical attribute edits through the CLI, with emit-grade
//! rejections for unknown keys, structural keys, missing and duplicate ids,
//! conflicting lines, and unrepresentable values.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]

use std::fs;
use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_vaire");

const TWO: &str = "[requirement#SWR-0001]\n[modality=shall, status=draft]\n[volatility=low]\n--\nThe system shall beep.\n--\n\n[requirement#SWR-0002]\n[modality=shall, status=draft]\n--\nThe system shall log.\n--\n";
/// `TWO` with SWR-0001's status edited to `active`.
const TWO_EDITED: &str = "[requirement#SWR-0001]\n[modality=shall, status=active]\n[volatility=low]\n--\nThe system shall beep.\n--\n\n[requirement#SWR-0002]\n[modality=shall, status=draft]\n--\nThe system shall log.\n--\n";
const QUOTED: &str = "[requirement#SWR-0500]\n[modality=shall, rationale=old, source=old]\n--\nThe system shall quote.\n--\n";

fn temp_source(dir: &Path, name: &str, contents: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    path.to_str().unwrap().to_owned()
}

fn run_edit(file: &str, id: &str, sets: &[&str], flags: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(BIN);
    cmd.args(["edit", file, id]);
    for set in sets {
        cmd.args(["--set", set]);
    }
    cmd.args(flags);
    cmd.output().unwrap()
}

fn stdout_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn setting_one_attribute_changes_only_its_line() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "surgical.adoc", TWO);
    let out = run_edit(&file, "SWR-0001", &["status=active"], &[]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("edited {file}: SWR-0001: status\n"),
        "summary line names file, id, and key"
    );
    let after = fs::read_to_string(&file).unwrap();
    assert_eq!(after, TWO_EDITED, "byte-diff is exactly the status line");
    let changed: Vec<usize> = TWO
        .lines()
        .zip(after.lines())
        .enumerate()
        .filter_map(|(i, (a, b))| (a != b).then_some(i))
        .collect();
    assert_eq!(changed, vec![1], "only SWR-0001's status line changed");
    assert!(after.contains("The system shall log.\n--\n"), "{after}");
}

#[test]
fn repeated_set_flags_edit_multiple_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "multi.adoc", TWO);
    let out = run_edit(
        &file,
        "SWR-0001",
        &["status=approved", "volatility=high"],
        &[],
    );
    assert!(out.status.success(), "{}", stderr_of(&out));
    let after = fs::read_to_string(&file).unwrap();
    assert_eq!(
        after,
        TWO.replacen(
            "[modality=shall, status=draft]",
            "[modality=shall, status=approved]",
            1,
        )
        .replace("[volatility=low]", "[volatility=high]")
    );
    assert_eq!(
        stdout_of(&out),
        format!("edited {file}: SWR-0001: status\nedited {file}: SWR-0001: volatility\n")
    );
}

#[test]
fn quoted_values_are_escaped_and_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "quoted.adoc", QUOTED);
    let out = run_edit(
        &file,
        "SWR-0500",
        &[r#"rationale=a"b, c]"#, "source=déjà vu ✓=x"],
        &[],
    );
    assert!(out.status.success(), "{}", stderr_of(&out));
    let written = fs::read_to_string(&file).unwrap();
    assert!(
        written.contains(r#"[modality=shall, rationale="a\"b, c]", source="déjà vu ✓=x"]"#),
        "values with quotes, commas, brackets, `=`, and unicode must be quoted and escaped: {written:?}"
    );
    let source = fs::read_to_string(&file).unwrap();
    let records = vaire::extract::extract(&file, &source).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].field("rationale").as_deref(), Some(r#"a"b, c]"#));
    assert_eq!(records[0].field("source").as_deref(), Some("déjà vu ✓=x"));
}

#[test]
fn an_unknown_key_is_rejected_and_the_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "unknown.adoc", TWO);
    let out = run_edit(&file, "SWR-0001", &["flavor=umami"], &[]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("flavor"), "{stderr}");
    assert!(stderr.contains("vocabulary"), "{stderr}");
    assert_eq!(fs::read(&file).unwrap(), TWO.as_bytes(), "file was written");
}

#[test]
fn a_known_key_absent_from_the_record_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "absent.adoc", TWO);
    let out = run_edit(&file, "SWR-0002", &["volatility=high"], &[]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("SWR-0002"), "{stderr}");
    assert!(stderr.contains("volatility"), "{stderr}");
    assert!(stderr.contains("carries no attribute"), "{stderr}");
    assert_eq!(fs::read(&file).unwrap(), TWO.as_bytes(), "file was written");
}

#[test]
fn a_missing_id_is_rejected_naming_the_id() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "missing.adoc", TWO);
    let out = run_edit(&file, "SWR-9999", &["status=active"], &[]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("SWR-9999"), "{stderr}");
    assert!(stderr.contains("not found"), "{stderr}");
    assert_eq!(fs::read(&file).unwrap(), TWO.as_bytes());
}

#[test]
fn a_duplicate_id_is_rejected_listing_locations() {
    let dir = tempfile::tempdir().unwrap();
    let dup = fs::read_to_string("tests/invalid/v1-duplicate-id.adoc").unwrap();
    let file = temp_source(dir.path(), "dup.adoc", &dup);
    let out = run_edit(&file, "SWR-0100", &["status=active"], &[]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("appears 2 times"), "{stderr}");
    assert!(stderr.contains("line 1 (byte 0)"), "{stderr}");
    assert!(stderr.contains("line 7 (byte"), "{stderr}");
    assert_eq!(fs::read(&file).unwrap(), dup.as_bytes());
}

#[test]
fn an_id_edit_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "id-edit.adoc", TWO);
    let out = run_edit(&file, "SWR-0001", &["id=SWR-9999"], &[]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("immutable"), "{stderr}");
    assert_eq!(fs::read(&file).unwrap(), TWO.as_bytes(), "file was written");
}

#[test]
fn a_style_edit_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "style-edit.adoc", TWO);
    let out = run_edit(&file, "SWR-0001", &["style=requirement"], &[]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("style"), "{stderr}");
    assert_eq!(fs::read(&file).unwrap(), TWO.as_bytes(), "file was written");
}

#[test]
fn a_body_edit_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "body-edit.adoc", TWO);
    let out = run_edit(&file, "SWR-0001", &["body=The system shall not beep."], &[]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("bodies are not editable"), "{stderr}");
    assert_eq!(fs::read(&file).unwrap(), TWO.as_bytes(), "file was written");
}

#[test]
fn setting_the_same_key_twice_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "dup-set.adoc", TWO);
    let out = run_edit(
        &file,
        "SWR-0001",
        &["status=active", "status=approved"],
        &[],
    );
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("more than once"), "{stderr}");
    assert_eq!(fs::read(&file).unwrap(), TWO.as_bytes(), "file was written");
}

#[test]
fn a_value_with_a_line_break_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "newline.adoc", TWO);
    for set in ["status=beep\nboop", "status=beep\rboop"] {
        let out = run_edit(&file, "SWR-0001", &[set], &[]);
        assert!(!out.status.success(), "{set}");
        let stderr = stderr_of(&out);
        assert!(stderr.contains("line break"), "{stderr}");
        assert!(stderr.contains("single-line"), "{stderr}");
        assert_eq!(
            fs::read(&file).unwrap(),
            TWO.as_bytes(),
            "rejected edit wrote the file"
        );
    }
}

#[test]
fn dry_run_prints_the_diff_and_leaves_the_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "dry.adoc", TWO);
    let out = run_edit(&file, "SWR-0001", &["status=active"], &["--dry-run"]);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    let stderr = stderr_of(&out);
    assert!(stderr.contains(&format!("--- {file}")), "{stderr}");
    assert!(stderr.contains("@ SWR-0001"), "{stderr}");
    assert!(
        stderr.contains("- [modality=shall, status=draft]"),
        "{stderr}"
    );
    assert!(
        stderr.contains("+ [modality=shall, status=active]"),
        "{stderr}"
    );
    assert_eq!(fs::read(&file).unwrap(), TWO.as_bytes(), "dry run wrote");
}

#[test]
fn diff_flag_prints_the_diff_and_writes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "diff.adoc", TWO);
    let out = run_edit(&file, "SWR-0001", &["status=active"], &["--diff"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("- [modality=shall, status=draft]"),
        "{stderr}"
    );
    assert!(
        stderr.contains("+ [modality=shall, status=active]"),
        "{stderr}"
    );
    assert_eq!(
        fs::read(&file).unwrap(),
        TWO_EDITED.as_bytes(),
        "diff mode wrote"
    );
    assert_eq!(
        stdout_of(&out),
        format!("edited {file}: SWR-0001: status\n")
    );
}

#[test]
fn a_repeated_identical_edit_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "noop.adoc", TWO);
    let first = run_edit(&file, "SWR-0001", &["status=active"], &[]);
    assert!(first.status.success(), "{}", stderr_of(&first));
    let before = fs::read(&file).unwrap();
    let mtime = fs::metadata(&file).unwrap().modified().unwrap();

    let second = run_edit(&file, "SWR-0001", &["status=active"], &[]);
    assert!(second.status.success());
    assert!(out_is_no_changes(&second), "{}", stderr_of(&second));
    assert!(second.stdout.is_empty());
    assert_eq!(fs::read(&file).unwrap(), before, "bytes changed");
    assert_eq!(
        fs::metadata(&file).unwrap().modified().unwrap(),
        mtime,
        "mtime changed"
    );
}

fn out_is_no_changes(out: &std::process::Output) -> bool {
    stderr_of(out).contains("no changes")
}

/// The write path `edit` uses re-verifies every planned region against the
/// live bytes right before splicing, so a line that changed after the record
/// set was built aborts the whole edit and leaves the file byte-identical.
#[test]
fn a_line_that_changed_after_the_records_were_built_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let file = temp_source(dir.path(), "stale.adoc", TWO);
    let json = {
        let source = fs::read_to_string(&file).unwrap();
        let mut records = vaire::extract::extract(&file, &source).unwrap();
        for line in &mut records[0].attr_lines {
            for item in &mut line.items {
                if let vaire::Item::Kv { name, value } = item
                    && name == "status"
                {
                    *value = "active".to_owned();
                }
            }
        }
        serde_json::to_string(&records).unwrap()
    };
    // Same-length rewrite, so the span still matches and the raw-line
    // comparison is what fires.
    let conflicting = TWO.replacen("status=draft", "status=dr@ft", 1);
    fs::write(&file, &conflicting).unwrap();

    let error = vaire::emit::emit(&json, &file).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("changed since"), "{message}");
    assert!(message.contains("re-extract"), "{message}");
    assert_eq!(
        fs::read(&file).unwrap(),
        conflicting.as_bytes(),
        "the conflicting edit was overwritten"
    );
}

#[test]
fn set_parse_splits_on_the_first_equals() {
    let set = vaire::edit::Set::parse("rationale=a=b=c").unwrap();
    assert_eq!(set.key, "rationale");
    assert_eq!(set.value, "a=b=c");
    let empty = vaire::edit::Set::parse("status=").unwrap();
    assert_eq!(empty.value, "");
    assert!(
        vaire::edit::Set::parse("novalue")
            .unwrap_err()
            .to_string()
            .contains("KEY=VALUE")
    );
    assert!(
        vaire::edit::Set::parse("=empty")
            .unwrap_err()
            .to_string()
            .contains("empty attribute name")
    );
}
