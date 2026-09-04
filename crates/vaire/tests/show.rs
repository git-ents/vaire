//! `show`: one requirement rendered for humans, with clear failures for
//! missing, duplicated, and unparsable sources — at the library boundary and
//! through the CLI binary.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]

use std::fs;
use std::process::Command;

use anstream::ColorChoice;

const BIN: &str = env!("CARGO_BIN_EXE_vaire");
const TRACES: &str = "tests/corpus/traces.adoc";
const CRLF: &str = "tests/corpus/crlf.adoc";
const DUPLICATES: &str = "tests/invalid/v1-duplicate-id.adoc";

const MULTILINE: &str = "[requirement#SWR-0001]\n[modality=shall]\n--\nThe system shall boot within 5 s.\n\n.Acceptance\n[.acceptance]\n* cold start\n* warm start\n--\n";
const QUOTED: &str = "[requirement#SWR-0500]\n[modality=shall, rationale=\"a\\\"b, c]\"]\n--\nThe system shall quote.\n--\n";

fn show_ok(file: &str, id: &str) -> String {
    let mut out = Vec::new();
    vaire::show::show(&mut out, file, id, ColorChoice::Never).unwrap();
    String::from_utf8(out).unwrap()
}

fn show_err(file: &str, id: &str) -> String {
    let mut sink = Vec::new();
    vaire::show::show(&mut sink, file, id, ColorChoice::Never)
        .unwrap_err()
        .to_string()
}

fn temp_source(dir: &tempfile::TempDir, name: &str, contents: &[u8]) -> String {
    let path = dir.path().join(name);
    fs::write(&path, contents).unwrap();
    path.to_str().unwrap().to_owned()
}

/// 1-based line number of `offset`, counted independently of the library.
fn line_of(source: &str, offset: usize) -> usize {
    1 + source.as_bytes()[..offset]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

#[test]
fn corpus_record_shows_every_section_in_order() {
    let out = show_ok(TRACES, "SWR-0004");
    assert!(out.contains("--- tests/corpus/traces.adoc"), "{out}");
    assert!(out.contains("id: SWR-0004"), "{out}");
    assert!(out.contains("style: requirement"), "{out}");
    assert!(out.contains("source: lines 8-14 (bytes "), "{out}");
    assert!(out.contains("delim: --"), "{out}");
    assert!(out.contains("  [requirement#SWR-0004]"), "{out}");
    assert!(
        out.contains(
            "  [rationale=\"safety interlock must respond before motion\", allocated-to=safety]"
        ),
        "{out}"
    );
    assert!(out.contains("modality = shall"), "{out}");
    assert!(out.contains("kind = functional"), "{out}");
    assert!(out.contains("verification = demonstration"), "{out}");
    assert!(
        out.contains("rationale = safety interlock must respond before motion"),
        "{out}"
    );
    assert!(out.contains("refines = SWR-0005"), "{out}");
    assert!(
        out.contains("The interlock shall stop motion within 50 ms of a trip signal.\n"),
        "{out}"
    );
    let raw = out.find("raw attribute lines:").unwrap();
    let parsed = out.find("parsed attributes:").unwrap();
    let traces = out.find("traces:").unwrap();
    let body = out.find("body:").unwrap();
    assert!(raw < parsed && parsed < traces && traces < body, "{out}");
}

#[test]
fn source_location_matches_the_record_span() {
    let source = fs::read_to_string(TRACES).unwrap();
    let records = vaire::extract::extract(TRACES, &source).unwrap();
    let record = records.iter().find(|r| r.id == "SWR-0004").unwrap();
    let expected = format!(
        "source: lines {}-{} (bytes {}-{})",
        line_of(&source, record.span.start),
        line_of(&source, record.span.end),
        record.span.start,
        record.span.end
    );
    assert!(show_ok(TRACES, "SWR-0004").contains(&expected));
}

#[test]
fn every_trace_key_is_listed_and_absent_traces_are_omitted() {
    let out = show_ok(TRACES, "SWR-0007");
    assert!(out.contains("satisfies = SWR-0005"), "{out}");
    assert!(out.contains("derives-from = SWR-0004"), "{out}");
    assert!(!show_ok(TRACES, "SWR-0005").contains("traces:"));
}

#[test]
fn missing_id_fails_naming_file_and_id() {
    let err = show_err(TRACES, "SWR-9999");
    assert!(err.contains("traces.adoc"), "{err}");
    assert!(err.contains("SWR-9999"), "{err}");
    assert!(err.contains("not found"), "{err}");
}

#[test]
fn duplicate_id_fails_listing_both_locations() {
    let err = show_err(DUPLICATES, "SWR-0100");
    assert!(err.contains("appears 2 times"), "{err}");
    assert!(err.contains("line 1 (byte 0)"), "{err}");
    assert!(err.contains("line 7 (byte"), "{err}");
}

#[test]
fn multiline_body_is_shown_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_source(&dir, "multiline.adoc", MULTILINE.as_bytes());
    let body = "body:\nThe system shall boot within 5 s.\n\n.Acceptance\n[.acceptance]\n* cold start\n* warm start\n";
    assert!(show_ok(&path, "SWR-0001").contains(body));
}

#[test]
fn quoted_attribute_values_show_raw_and_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_source(&dir, "quoted.adoc", QUOTED.as_bytes());
    let out = show_ok(&path, "SWR-0500");
    assert!(
        out.contains("  [modality=shall, rationale=\"a\\\"b, c]\"]"),
        "{out}"
    );
    assert!(out.contains("rationale = a\"b, c]"), "{out}");
}

#[test]
fn crlf_source_shows_the_body_verbatim() {
    let out = show_ok(CRLF, "SWR-0400");
    assert!(out.contains("id: SWR-0400"), "{out}");
    assert!(out.contains("source: lines 1-"), "{out}");
    assert!(
        out.contains("The joint controller shall publish a sample within 2 ms.\r\n"),
        "{out}"
    );
    assert!(out.contains("nested example content\r\n"), "{out}");
    assert!(!out.contains("[requirement#SWR-0400]\r"), "{out}");
}

#[test]
fn forced_color_emits_ansi_and_plain_choice_does_not() {
    let mut plain = Vec::new();
    vaire::show::show(&mut plain, TRACES, "SWR-0004", ColorChoice::Never).unwrap();
    assert!(!plain.contains(&0x1b), "ESC byte in plain output");
    let mut colored = Vec::new();
    vaire::show::show(&mut colored, TRACES, "SWR-0004", ColorChoice::Always).unwrap();
    assert!(colored.contains(&0x1b), "no ANSI in forced-color output");
}

#[test]
fn invalid_source_fails_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_source(
        &dir,
        "unterminated.adoc",
        b"[requirement#SWR-0001]\n--\nunterminated\n",
    );
    let err = show_err(&path, "SWR-0001");
    assert!(err.contains("unterminated.adoc"), "{err}");
    assert!(err.contains("parse error"), "{err}");
}

#[test]
fn missing_file_fails_naming_the_path() {
    let err = show_err("/nonexistent/vaire/show.adoc", "SWR-0001");
    assert!(err.contains("/nonexistent/vaire/show.adoc"), "{err}");
}

#[test]
fn non_utf8_source_fails_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_source(
        &dir,
        "binary.adoc",
        b"[requirement#SWR-0001]\n--\n\xff\xfe\n--\n",
    );
    let err = show_err(&path, "SWR-0001");
    assert!(err.contains("binary.adoc"), "{err}");
    assert!(err.contains("UTF-8"), "{err}");
}

#[test]
fn cli_show_piped_writes_plain_data_to_stdout() {
    let out = Command::new(BIN)
        .args(["show", TRACES, "SWR-0004"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!out.stdout.contains(&0x1b), "ESC byte in piped stdout");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("id: SWR-0004"), "{stdout}");
    assert!(stdout.contains("refines = SWR-0005"), "{stdout}");
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_show_missing_id_exits_nonzero_reporting_on_stderr() {
    let out = Command::new(BIN)
        .args(["show", TRACES, "SWR-9999"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("SWR-9999"), "{stderr}");
    assert!(stderr.contains("traces.adoc"), "{stderr}");
}
