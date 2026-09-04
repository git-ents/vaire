//! `check`: violation lines, summaries, machine-readable reports, and rule
//! filtering through the CLI binary.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]

use std::fs;
use std::process::{Command, Output};

use serde::Deserialize;
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_vaire");

/// One valid leaf requirement.
const VALID: &str = "[requirement#SWR-0001]\n[modality=shall, kind=functional, verification=test]\n--\nThe system shall boot within 5 s.\n--\n";

/// An unresolved trace target: fails V2 and nothing else.
const UNRESOLVED: &str = "[requirement#SWR-0101]\n[modality=shall, kind=functional, verification=test]\n[refines=SWR-9999]\n--\nThe system shall persist faults across restarts.\n--\n";

/// Prose modality disagrees with the attribute: fails V4 and nothing else.
const DISAGREE: &str = "[requirement#SWR-0103]\n[modality=shall, kind=functional, verification=test]\n--\nThe system may retry dropped samples.\n--\n";

/// Four independent records failing V2, V3, V4, and V6, in that order.
const MIXED: &str = "[requirement#SWR-0101]\n[modality=shall, kind=functional, verification=test]\n[refines=SWR-9999]\n--\nThe system shall persist faults across restarts.\n--\n\n[requirement#SWR-0102]\n[modality=shall, kind=functional]\n--\nThe system shall time-stamp every fault.\n--\n\n[requirement#SWR-0103]\n[modality=shall, kind=functional, verification=test]\n--\nThe system may retry dropped samples.\n--\n\n[requirement#SWR-0105]\n[modality=shall, kind=functional, verification=test]\n[priority=high]\n--\nThe system shall degrade gracefully under load.\n--\n";

/// The summary `--summary` must print for `MIXED`.
const MIXED_SUMMARY: &str = "checked 1 file, 4 requirements, 4 violations\nviolations by rule: V1=0 V2=1 V3=1 V4=1 V5=0 V6=1\n";

/// A parent without verification: it is refined by [`CHILD`].
const PARENT: &str = "[requirement#SWR-0005]\n[modality=shall, kind=functional]\n--\nThe controller shall log every fault.\n--\n";

/// A child refining [`PARENT`].
const CHILD: &str = "[requirement#SWR-0004]\n[modality=shall, kind=functional, verification=test]\n[refines=SWR-0005]\n--\nThe interlock shall stop motion within 50 ms.\n--\n";

/// The same requirement body, to be defined twice across two files.
const SHARED: &str = "[requirement#SWR-DUP]\n[modality=shall, kind=functional, verification=test]\n--\nThe system shall log all faults.\n--\n";

/// A requirement tracing to its own id.
const SELF_TRACED: &str = "[requirement#SELF-1]\n[modality=shall, kind=functional, verification=test]\n[refines=SELF-1]\n--\nThe module shall validate its own inputs.\n--\n";

/// Two requirements refining each other.
const CYCLE: &str = "[requirement#CYC-A]\n[modality=shall, kind=functional]\n[refines=CYC-B]\n--\nThe system shall debounce encoder inputs.\n--\n\n[requirement#CYC-B]\n[modality=shall, kind=functional]\n[refines=CYC-A]\n--\nThe system shall cache encoder reads.\n--\n";

/// Two records; the second (starting on line 7) disagrees on modality.
const TWO_RECORDS: &str = "[requirement#SWR-0001]\n[modality=shall, kind=functional, verification=test]\n--\nThe system shall boot.\n--\n\n[requirement#SWR-0002]\n[modality=shall, kind=functional, verification=test]\n--\nThe system may lag.\n--\n";

/// One violation as exposed by `check`'s machine-readable formats.
#[derive(Debug, Deserialize)]
struct Row {
    rule: String,
    file: String,
    id: String,
    message: String,
    line: Option<usize>,
    offset: usize,
}

/// The machine-readable `check` report.
#[derive(Debug, Deserialize)]
struct Report {
    file_count: usize,
    requirement_count: usize,
    violation_count: usize,
    violations: Vec<Row>,
}

fn run(files: &[&str], flags: &[&str]) -> Output {
    Command::new(BIN)
        .arg("check")
        .args(files)
        .args(flags)
        .output()
        .unwrap()
}

/// Run `check` with color forced on, as if attached to a color terminal.
fn run_forced(files: &[&str], flags: &[&str]) -> Output {
    Command::new(BIN)
        .arg("check")
        .args(files)
        .args(flags)
        .env("CLICOLOR_FORCE", "1")
        .output()
        .unwrap()
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn report_of(out: &Output) -> Report {
    serde_json::from_str(&stdout_of(out)).unwrap()
}

fn temp_source(dir: &TempDir, name: &str, contents: &str) -> String {
    let path = dir.path().join(name);
    fs::write(&path, contents).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn each_invalid_fixture_fails_with_its_rule_line() {
    for (code, name) in [
        ("V1", "v1-duplicate-id.adoc"),
        ("V2", "v2-unresolved-trace.adoc"),
        ("V3", "v3-missing-verification.adoc"),
        ("V4", "v4-modality-disagreement.adoc"),
        ("V5", "v5-compound-statement.adoc"),
        ("V6", "v6-unknown-attribute.adoc"),
    ] {
        let out = run(&[&format!("tests/invalid/{name}")], &[]);
        assert!(!out.status.success(), "{name}: exited success");
        let stderr = stderr_of(&out);
        assert!(stderr.starts_with(&format!("{code}: ")), "{name}: {stderr}");
        assert_eq!(stderr.lines().count(), 1, "{name}: {stderr}");
        assert!(out.stdout.is_empty(), "{name}: {:?}", out.stdout);
    }
}

#[test]
fn clean_corpus_is_silent_and_successful() {
    let out = run(
        &[
            "tests/corpus/seed.adoc",
            "tests/corpus/traces.adoc",
            "tests/corpus/crlf.adoc",
        ],
        &[],
    );
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}

#[test]
fn violation_output_is_deterministic_and_ordered() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let first = run(&[&mixed], &[]);
    let second = run(&[&mixed], &[]);
    assert!(!first.status.success());
    assert_eq!(first.stderr, second.stderr, "byte-identical across runs");
    assert_eq!(first.stdout, second.stdout);
    let stderr = stderr_of(&first);
    assert!(
        !stderr.contains('\x1b'),
        "piped stderr is not colored: {stderr:?}"
    );
    let at = |code: &str| stderr.find(&format!("{code}: ")).unwrap();
    assert!(
        at("V2") < at("V3") && at("V3") < at("V4") && at("V4") < at("V6"),
        "{stderr}"
    );
    assert_eq!(stderr.lines().count(), 4, "{stderr}");
}

#[test]
fn violations_follow_file_argument_order() {
    let dir = TempDir::new().unwrap();
    let a = temp_source(&dir, "a.adoc", DISAGREE);
    let b = temp_source(&dir, "b.adoc", UNRESOLVED);
    let a_then_b = stderr_of(&run(&[&a, &b], &[]));
    let b_then_a = stderr_of(&run(&[&b, &a], &[]));
    assert!(
        a_then_b.find("V4: ").unwrap() < a_then_b.find("V2: ").unwrap(),
        "{a_then_b}"
    );
    assert!(
        b_then_a.find("V2: ").unwrap() < b_then_a.find("V4: ").unwrap(),
        "{b_then_a}"
    );
}

#[test]
fn cross_file_traces_resolve_across_the_whole_set() {
    let dir = TempDir::new().unwrap();
    let parent = temp_source(&dir, "parent.adoc", PARENT);
    let child = temp_source(&dir, "child.adoc", CHILD);
    let both = run(&[&parent, &child], &[]);
    assert!(both.status.success(), "{}", stderr_of(&both));
    let child_alone = run(&[&child], &[]);
    assert!(!child_alone.status.success());
    assert!(
        stderr_of(&child_alone).starts_with("V2: "),
        "{}",
        stderr_of(&child_alone)
    );
    let parent_alone = run(&[&parent], &[]);
    assert!(!parent_alone.status.success());
    assert!(
        stderr_of(&parent_alone).starts_with("V3: "),
        "{}",
        stderr_of(&parent_alone)
    );
}

#[test]
fn duplicate_ids_across_files_report_on_the_later_file() {
    let dir = TempDir::new().unwrap();
    let one = temp_source(&dir, "one.adoc", SHARED);
    let two = temp_source(&dir, "two.adoc", SHARED);
    for (first, second) in [(&one, &two), (&two, &one)] {
        let out = run(&[first.as_str(), second.as_str()], &[]);
        assert!(!out.status.success());
        let stderr = stderr_of(&out);
        assert!(stderr.starts_with("V1: "), "{stderr}");
        assert!(
            stderr.contains(second.as_str()),
            "the later file carries the violation: {stderr}"
        );
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
}

#[test]
fn self_traces_resolve_but_do_not_parent() {
    let dir = TempDir::new().unwrap();
    let verified = temp_source(&dir, "self.adoc", SELF_TRACED);
    let out = run(&[&verified], &[]);
    assert!(out.status.success(), "{}", stderr_of(&out));

    let unverified = SELF_TRACED.replace(", verification=test", "");
    let path = temp_source(&dir, "self-unverified.adoc", &unverified);
    let out = run(&[&path], &[]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(stderr.starts_with("V3: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
}

#[test]
fn two_requirement_cycles_are_not_detected() {
    let dir = TempDir::new().unwrap();
    let cycle = temp_source(&dir, "cycle.adoc", CYCLE);
    let out = run(&[&cycle], &[]);
    assert!(out.status.success(), "{}", stderr_of(&out));
}

#[test]
fn each_unknown_attribute_is_its_own_v6() {
    let dir = TempDir::new().unwrap();
    let src = "[requirement#SWR-0105]\n[modality=shall, kind=functional, verification=test]\n[priority=high, severity=low]\n--\nThe system shall degrade gracefully under load.\n--\n";
    let path = temp_source(&dir, "two-unknown.adoc", src);
    let out = run(&[&path], &[]);
    let stderr = stderr_of(&out);
    assert_eq!(stderr.lines().count(), 2, "{stderr}");
    assert!(stderr.contains("unknown attribute `priority`"), "{stderr}");
    assert!(stderr.contains("unknown attribute `severity`"), "{stderr}");
    assert!(
        stderr.find("`priority`").unwrap() < stderr.find("`severity`").unwrap(),
        "{stderr}"
    );
}

#[test]
fn missing_files_report_the_library_io_error() {
    let out = run(&["no-such-file.adoc"], &[]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(
        stderr_of(&out).starts_with("vaire: io error:"),
        "{}",
        stderr_of(&out)
    );
}

#[test]
fn json_report_parses_and_never_carries_ansi() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let out = run_forced(&[&mixed], &["--format", "json"]);
    assert!(!out.status.success());
    assert!(out.stderr.is_empty(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(!stdout.contains('\x1b'), "{stdout:?}");
    let report: Report = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report.file_count, 1);
    assert_eq!(report.requirement_count, 4);
    assert_eq!(report.violation_count, 4);
    assert_eq!(
        report
            .violations
            .iter()
            .map(|row| row.rule.as_str())
            .collect::<Vec<_>>(),
        ["V2", "V3", "V4", "V6"]
    );
    assert!(report.violations.iter().all(|row| row.file == mixed));
    assert!(report.violations.iter().all(|row| row.line.is_some()));
}

#[test]
fn compact_json_is_one_line_with_the_same_object() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let compact = stdout_of(&run(&[&mixed], &["--format", "compact"]));
    assert_eq!(compact.lines().count(), 1, "{compact:?}");
    let compact: Report = serde_json::from_str(&compact).unwrap();
    let pretty = report_of(&run(&[&mixed], &["--format", "json"]));
    assert_eq!(compact.violation_count, pretty.violation_count);
    assert_eq!(compact.violations.len(), pretty.violations.len());
}

#[test]
fn human_lines_stay_colored_when_color_is_forced() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let out = run_forced(&[&mixed], &[]);
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains('\x1b'),
        "forced color styles violation lines: {:?}",
        stderr_of(&out)
    );
}

#[test]
fn clean_files_report_zero_violations_in_json() {
    let dir = TempDir::new().unwrap();
    let valid = temp_source(&dir, "valid.adoc", VALID);
    let out = run(&[&valid], &["--format", "json"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let report = report_of(&out);
    assert_eq!(report.file_count, 1);
    assert_eq!(report.requirement_count, 1);
    assert_eq!(report.violation_count, 0);
    assert!(report.violations.is_empty());
}

#[test]
fn quiet_suppresses_lines_but_keeps_the_exit_code() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let out = run(&[&mixed], &["-q"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}

#[test]
fn quiet_summary_prints_only_the_summary() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let out = run(&[&mixed], &["-q", "--summary"]);
    assert!(!out.status.success());
    assert!(out.stderr.is_empty());
    assert_eq!(stdout_of(&out), MIXED_SUMMARY);
}

#[test]
fn summary_counts_a_mixed_file() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let out = run(&[&mixed], &["--summary"]);
    assert!(!out.status.success());
    assert_eq!(stdout_of(&out), MIXED_SUMMARY);
    assert_eq!(stderr_of(&out).lines().count(), 4);
}

#[test]
fn summary_on_a_clean_file_reports_zero() {
    let dir = TempDir::new().unwrap();
    let valid = temp_source(&dir, "valid.adoc", VALID);
    let out = run(&[&valid], &["--summary"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "checked 1 file, 1 requirement, 0 violations\nviolations by rule: V1=0 V2=0 V3=0 V4=0 V5=0 V6=0\n"
    );
    assert!(out.stderr.is_empty());
}

#[test]
fn rule_filter_limits_display_but_not_the_exit_code() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let out = run(&[&mixed], &["--rule", "V4"]);
    assert!(!out.status.success(), "unselected rules still fail the run");
    let stderr = stderr_of(&out);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("V4: "), "{stderr}");

    let none = run(&[&mixed], &["--rule", "V1"]);
    assert!(
        !none.status.success(),
        "the filter never silences the exit code"
    );
    assert!(none.stderr.is_empty() && none.stdout.is_empty());
}

#[test]
fn rule_filter_is_repeatable() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let out = run(&[&mixed], &["--rule", "V4", "--rule", "V6"]);
    let stderr = stderr_of(&out);
    assert_eq!(stderr.lines().count(), 2, "{stderr}");
    assert!(
        stderr.find("V4: ").unwrap() < stderr.find("V6: ").unwrap(),
        "{stderr}"
    );
}

#[test]
fn rule_filter_applies_to_json_but_counts_stay_total() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let out = run(&[&mixed], &["--format", "json", "--rule", "V4"]);
    assert!(!out.status.success());
    let report = report_of(&out);
    assert_eq!(report.violation_count, 4);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].rule, "V4");
    assert_eq!(report.violations[0].id, "SWR-0103");
}

#[test]
fn quiet_leaves_the_json_report_untouched() {
    let dir = TempDir::new().unwrap();
    let mixed = temp_source(&dir, "mixed.adoc", MIXED);
    let out = run(&[&mixed], &["-q", "--format", "json"]);
    assert!(!out.status.success());
    assert!(out.stderr.is_empty());
    assert_eq!(report_of(&out).violation_count, 4);
}

#[test]
fn crlf_files_report_the_same_lines_as_lf() {
    let dir = TempDir::new().unwrap();
    let lf = temp_source(&dir, "lf.adoc", TWO_RECORDS);
    let crlf = temp_source(&dir, "crlf.adoc", &TWO_RECORDS.replace('\n', "\r\n"));
    let lf_report = report_of(&run(&[&lf], &["--format", "json"]));
    let crlf_report = report_of(&run(&[&crlf], &["--format", "json"]));
    assert_eq!(lf_report.violations.len(), 1);
    assert_eq!(crlf_report.violations.len(), 1);
    let (lf_row, crlf_row) = (&lf_report.violations[0], &crlf_report.violations[0]);
    assert_eq!(lf_row.rule, "V4");
    assert_eq!(lf_row.id, "SWR-0002");
    assert_eq!(lf_row.message, crlf_row.message);
    assert_eq!(lf_row.line, crlf_row.line);
    assert_eq!(lf_row.line, Some(7));
    let offset = TWO_RECORDS.find("[requirement#SWR-0002]").unwrap();
    let terminators = TWO_RECORDS.get(..offset).unwrap().matches('\n').count();
    assert_eq!(crlf_row.offset, lf_row.offset + terminators);
}

#[test]
fn summary_and_format_are_mutually_exclusive() {
    let dir = TempDir::new().unwrap();
    let valid = temp_source(&dir, "valid.adoc", VALID);
    let out = run(&[&valid], &["--summary", "--format", "json"]);
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains("cannot be used with"),
        "{}",
        stderr_of(&out)
    );
}
