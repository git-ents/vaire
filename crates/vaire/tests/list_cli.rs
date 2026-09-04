//! `list`: filters, ordering, and machine-readable formats through the CLI
//! binary, with the human table unchanged as the default output.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde::Deserialize;

use vaire::Span;

const BIN: &str = env!("CARGO_BIN_EXE_vaire");
const TRACES: &str = "tests/corpus/traces.adoc";
const MISC: &str = "tests/corpus/misc.adoc";
const TABLE_HEADER: &str = "ID  KIND  MODALITY  STATUS  VERIFICATION";

/// One requirement with every column populated, for JSON field checks.
const SINGLE: &str = "[requirement#SWR-0001]\n[modality=shall, kind=functional, status=draft]\n[verification=test]\n--\nThe system shall boot.\n--\n";

/// A record shared by two fixtures to exercise duplicate ids across files.
const SHARED: &str = "[requirement#SWR-0100]\n[modality=shall, kind=functional, verification=test]\n--\nThe system shall log all faults.\n--\n";

/// One record whose `satisfies` value lists two ids in one quoted value.
const COMMA_TRACE: &str = "[requirement#SWR-0600]\n[modality=shall, satisfies=\"SWR-0005, SWR-0006\"]\n--\nThe system shall trace to both.\n--\n";

/// A machine-readable `list` row, mirroring the documented JSON schema.
#[derive(Debug, Deserialize, PartialEq)]
struct ListRow {
    file: String,
    id: String,
    span: Span,
    modality: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    verification: Option<String>,
    statement: String,
}

fn run(files: &[&str], flags: &[&str]) -> Output {
    Command::new(BIN)
        .arg("list")
        .args(files)
        .args(flags)
        .output()
        .unwrap()
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn rows_of(out: &Output) -> Vec<ListRow> {
    serde_json::from_str(&stdout_of(out)).unwrap()
}

fn temp_source(dir: &Path, name: &str, contents: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    path.to_str().unwrap().to_owned()
}

/// Number of table rows whose id column equals `id`.
fn table_rows(out: &str, id: &str) -> usize {
    out.lines()
        .filter(|line| {
            line.strip_prefix(id)
                .is_some_and(|rest| rest.starts_with("  "))
        })
        .count()
}

#[test]
fn one_file_lists_records_in_source_order() {
    let out = run(&[TRACES], &[]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert!(out.stderr.is_empty(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("--- {TRACES}")), "{stdout}");
    assert!(stdout.contains(TABLE_HEADER), "{stdout}");
    let pos = |id: &str| stdout.find(&format!("{id}  ")).unwrap();
    assert!(pos("SWR-0005") < pos("SWR-0004"), "{stdout}");
    assert!(pos("SWR-0004") < pos("SWR-0007"), "{stdout}");
    assert!(
        stdout.contains("The controller shall log every fault"),
        "{stdout}"
    );
}

#[test]
fn file_argument_order_is_stable_across_runs() {
    let first = run(&[MISC, TRACES], &[]);
    let second = run(&[MISC, TRACES], &[]);
    assert!(first.status.success(), "{}", stderr_of(&first));
    assert_eq!(first.stdout, second.stdout, "byte-identical across runs");
    let stdout = stdout_of(&first);
    assert!(
        stdout.find(&format!("--- {MISC}")).unwrap()
            < stdout.find(&format!("--- {TRACES}")).unwrap()
    );
    let reversed = stdout_of(&run(&[TRACES, MISC], &[]));
    assert!(
        reversed.find(&format!("--- {TRACES}")).unwrap()
            < reversed.find(&format!("--- {MISC}")).unwrap(),
        "argument order decides section order: {reversed}"
    );
}

#[test]
fn empty_result_prints_headers_or_empty_array() {
    let out = run(&[TRACES], &["--id", "NOPE"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(TABLE_HEADER), "headers only: {stdout}");
    assert!(!stdout.contains("SWR-"), "{stdout}");
    for format in ["json", "compact", "tsv"] {
        let out = run(&[TRACES], &["--id", "NOPE", "--format", format]);
        assert!(out.status.success());
        let stdout = stdout_of(&out);
        if format == "tsv" {
            assert_eq!(stdout.lines().count(), 1, "header row only: {stdout:?}");
        } else {
            assert_eq!(stdout.trim_end(), "[]", "{stdout}");
        }
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn id_prefix_filter_matches_one_record() {
    let out = run(&[TRACES], &["--id", "SWR-0004"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert_eq!(table_rows(&stdout, "SWR-0004"), 1, "{stdout}");
    assert!(
        stdout.contains("The interlock shall stop motion"),
        "{stdout}"
    );
}

#[test]
fn id_prefix_filter_matches_many() {
    let stdout = stdout_of(&run(&[MISC], &["--id", "SWR-00"]));
    assert_eq!(table_rows(&stdout, "SWR-0001"), 1, "{stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0002"), 1, "{stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0003"), 1, "{stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0006"), 1, "{stdout}");
}

#[test]
fn filters_combine_with_and_semantics() {
    let stdout = stdout_of(&run(&[MISC], &["--modality", "shall", "--status", "draft"]));
    assert_eq!(table_rows(&stdout, "SWR-0006"), 1, "{stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0001"), 0, "{stdout}");
    let stdout = stdout_of(&run(
        &[MISC],
        &[
            "--attr",
            "kind=functional",
            "--attr",
            "verification=analysis",
        ],
    ));
    assert_eq!(table_rows(&stdout, "SWR-0006"), 1, "{stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0001"), 0, "{stdout}");
    let stdout = stdout_of(&run(&[MISC, TRACES], &["--verification", "inspection"]));
    assert_eq!(table_rows(&stdout, "SWR-0002"), 1, "{stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0005"), 0, "{stdout}");
}

#[test]
fn unknown_filter_value_errors_listing_valid_values() {
    let out = run(&[TRACES], &["--modality", "maybe"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "{}", stdout_of(&out));
    let stderr = stderr_of(&out);
    assert!(stderr.contains("maybe"), "{stderr}");
    for valid in ["shall", "should", "may"] {
        assert!(stderr.contains(valid), "{stderr}");
    }
}

#[test]
fn malformed_attr_errors_on_stderr() {
    let out = run(&[TRACES], &["--attr", "kind"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(stderr_of(&out).contains("KEY=VALUE"), "{}", stderr_of(&out));
}

#[test]
fn case_sensitive_exact_filters_do_not_forgive_case() {
    let out = run(&[MISC], &["--status", "Draft"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(TABLE_HEADER), "{stdout}");
    assert!(!stdout.contains("SWR-0006"), "{stdout}");
    let out = run(&[MISC], &["--modality", "SHALL"]);
    assert!(!out.status.success(), "modality is case-sensitive too");
}

#[test]
fn duplicate_ids_across_files_are_both_listed() {
    let dir = tempfile::tempdir().unwrap();
    let a = temp_source(dir.path(), "a.adoc", SHARED);
    let b = temp_source(dir.path(), "b.adoc", SHARED);
    let rows = rows_of(&run(&[&a, &b], &["--format", "json"]));
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].id, "SWR-0100");
    assert_eq!(rows[1].id, "SWR-0100");
    assert_ne!(rows[0].file, rows[1].file, "file disambiguates");
    assert!(rows.iter().any(|r| r.file == a), "{rows:?}");
    assert!(rows.iter().any(|r| r.file == b), "{rows:?}");
    let stdout = stdout_of(&run(&[&a, &b], &[]));
    assert_eq!(table_rows(&stdout, "SWR-0100"), 2, "{stdout}");
    assert!(
        stdout.find(&format!("--- {a}")).unwrap() < stdout.find(&format!("--- {b}")).unwrap(),
        "{stdout}"
    );
}

#[test]
fn json_format_has_stable_documented_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_source(dir.path(), "single.adoc", SINGLE);
    let out = run(&[&path], &["--format", "json"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let rows = rows_of(&out);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let source = fs::read_to_string(&path).unwrap();
    let record = vaire::extract::extract(&path, &source).unwrap().remove(0);
    assert_eq!(
        rows[0],
        ListRow {
            file: path,
            id: "SWR-0001".to_owned(),
            span: record.span,
            modality: Some("shall".to_owned()),
            kind: Some("functional".to_owned()),
            status: Some("draft".to_owned()),
            verification: Some("test".to_owned()),
            statement: "The system shall boot.".to_owned(),
        }
    );
}

#[test]
fn json_absent_attributes_are_null_and_order_follows_source() {
    let rows = rows_of(&run(&[TRACES], &["--format", "json"]));
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["SWR-0005", "SWR-0004", "SWR-0007"], "{rows:?}");
    assert_eq!(rows[0].file, TRACES);
    let swr5 = rows.iter().find(|r| r.id == "SWR-0005").unwrap();
    assert_eq!(swr5.status, None, "SWR-0005 carries no status attribute");
}

#[test]
fn compact_format_is_one_line_of_valid_json() {
    let out = run(&[TRACES], &["--format", "compact"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert_eq!(stdout.lines().count(), 1, "single line: {stdout:?}");
    let rows: Vec<ListRow> = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!(rows, rows_of(&run(&[TRACES], &["--format", "json"])));
}

#[test]
fn tsv_format_has_a_header_and_one_row_per_record() {
    let out = run(&[TRACES], &["--format", "tsv"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    let mut lines = stdout.lines();
    assert_eq!(
        lines.next(),
        Some("file\tid\tspan_start\tspan_end\tmodality\tkind\tstatus\tverification\tstatement"),
        "{stdout}"
    );
    assert_eq!(lines.count(), 3, "{stdout}");
    for line in stdout.lines().skip(1) {
        assert_eq!(line.matches('\t').count(), 8, "{line}");
    }
}

#[test]
fn piped_output_has_no_ansi_escapes_in_any_format() {
    let formats: [&[&str]; 4] = [
        &[],
        &["--format", "json"],
        &["--format", "compact"],
        &["--format", "tsv"],
    ];
    for flags in formats {
        let out = run(&[TRACES], flags);
        assert!(out.status.success(), "{}", stderr_of(&out));
        assert!(!out.stdout.contains(&0x1b), "ESC byte in {flags:?} output");
        assert!(out.stderr.is_empty(), "{}", stderr_of(&out));
    }
    assert!(stdout_of(&run(&[TRACES], &[])).contains(TABLE_HEADER));
}

#[test]
fn traces_filter_matches_merged_trace_keys() {
    let stdout = stdout_of(&run(&[TRACES], &["--traces", "SWR-0005"]));
    assert_eq!(table_rows(&stdout, "SWR-0004"), 1, "refines: {stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0007"), 1, "satisfies: {stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0005"), 0, "{stdout}");
    let stdout = stdout_of(&run(&[TRACES], &["--traces", "SWR-0004"]));
    assert_eq!(table_rows(&stdout, "SWR-0007"), 1, "derives-from: {stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0004"), 0, "{stdout}");
    let stdout = stdout_of(&run(
        &[TRACES],
        &["--traces", "SWR-0005", "--traces", "SWR-0004"],
    ));
    assert_eq!(table_rows(&stdout, "SWR-0007"), 1, "AND: {stdout}");
    assert_eq!(table_rows(&stdout, "SWR-0004"), 0, "{stdout}");
}

#[test]
fn traces_filter_counts_comma_separated_field_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_source(dir.path(), "comma.adoc", COMMA_TRACE);
    let stdout = stdout_of(&run(&[&path], &["--traces", "SWR-0006"]));
    assert_eq!(table_rows(&stdout, "SWR-0600"), 1, "{stdout}");
}

#[test]
fn file_flag_restricts_output_and_rejects_unknown_paths() {
    let out = run(&[MISC, TRACES], &["--file", TRACES]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("--- {TRACES}")), "{stdout}");
    assert!(!stdout.contains(&format!("--- {MISC}")), "{stdout}");
    let rows = rows_of(&run(
        &[MISC, TRACES],
        &["--file", TRACES, "--format", "json"],
    ));
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r.file == TRACES), "{rows:?}");
    let out = run(&[MISC, TRACES], &["--file", "nope.adoc"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let stderr = stderr_of(&out);
    assert!(stderr.contains("nope.adoc"), "{stderr}");
    assert!(stderr.contains("misc.adoc"), "{stderr}");
    assert!(stderr.contains("traces.adoc"), "{stderr}");
}

#[test]
fn paths_with_spaces_list_and_filter() {
    let dir = tempfile::tempdir().unwrap();
    let spaced = temp_source(dir.path(), "my reqs.adoc", SINGLE);
    let other = temp_source(dir.path(), "plain.adoc", SHARED);
    let out = run(&[&spaced, &other], &[]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("--- {spaced}")), "{stdout}");
    let rows = rows_of(&run(&[&spaced, &other], &["--format", "json"]));
    assert!(rows.iter().any(|r| r.file == spaced), "{rows:?}");
    let rows = rows_of(&run(
        &[&spaced, &other],
        &["--file", &spaced, "--format", "json"],
    ));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].file, spaced);
}
