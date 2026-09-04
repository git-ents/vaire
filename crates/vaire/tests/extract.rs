//! Extraction spans and delimiter handling: LF/CRLF parity, block shapes,
//! and the rejection rules of `spec/requirements-syntax.adoc` §2.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]

use std::fs;

use tempfile::TempDir;

fn fixture(name: &str) -> String {
    fs::read_to_string(format!("tests/extract/{name}")).unwrap()
}

fn extract(source: &str) -> vaire::Result<Vec<vaire::Record>> {
    vaire::extract::extract("t.adoc", source)
}

fn violations(source: &str) -> Vec<vaire::check::Violation> {
    vaire::check::validate(&extract(source).unwrap())
}

fn rules(v: &[vaire::check::Violation]) -> Vec<vaire::check::ValidationRule> {
    v.iter().map(|x| x.rule).collect()
}

/// The CRLF twin of `source`: every LF terminator becomes CRLF.
fn crlf(source: &str) -> String {
    source.replace('\n', "\r\n")
}

/// A minimal valid requirement, used to derive CRLF and edge-case variants.
const VALID: &str = "[requirement#SWR-0001]\n\
                     [modality=shall, kind=functional, verification=test]\n\
                     [status=draft]\n\
                     --\n\
                     The system shall beep.\n\
                     --\n";

/// LF and CRLF documents extract with identical attribute lines, delimiters,
/// and statements; only byte spans and the raw terminators differ.
#[test]
fn lf_and_clf_extract_with_identical_semantics() {
    let lf = fixture("valid-lf.adoc");
    let lf_records = extract(&lf).unwrap();
    let crlf_records = extract(&crlf(&lf)).unwrap();
    assert_eq!(lf_records.len(), 1);
    assert_eq!(crlf_records.len(), lf_records.len());
    for (lf_r, c_r) in lf_records.iter().zip(&crlf_records) {
        assert_eq!(lf_r.id, c_r.id);
        assert_eq!(lf_r.delim, c_r.delim);
        assert_eq!(lf_r.statement(), c_r.statement());
        assert_eq!(lf_r.attr_lines.len(), c_r.attr_lines.len());
        for (lf_l, c_l) in lf_r.attr_lines.iter().zip(&c_r.attr_lines) {
            assert_eq!(lf_l.raw, c_l.raw, "attribute line bytes must match");
            assert_eq!(lf_l.items, c_l.items);
        }
        assert_eq!(
            c_r.body_raw,
            lf_r.body_raw.replace('\n', "\r\n"),
            "body_raw carries the raw terminators"
        );
    }
}

#[test]
fn crlf_body_raw_keeps_crlf_terminators() {
    let records = extract(&fixture("valid-crlf.adoc")).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].body_raw, "The system shall beep.\r\n");
    assert_eq!(records[0].delim, "--");
    assert_eq!(records[0].statement(), "The system shall beep.");
}

#[test]
fn multiple_crlf_requirements_extract_in_source_order() {
    let source = fixture("multi-crlf.adoc");
    let records = extract(&source).unwrap();
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["SWR-0310", "SWR-0311", "SWR-0312"]);
    let bodies: Vec<&str> = records.iter().map(|r| r.body_raw.as_str()).collect();
    assert_eq!(
        bodies,
        [
            "The system shall boot within 5 s.\r\n",
            "The debug port may be disabled.\r\n",
            "Scale:: uninterrupted hours\r\nMeter:: soak test\r\nMust:: 500\r\n",
        ]
    );
    for pair in records.windows(2) {
        assert!(pair[0].span.end <= pair[1].span.start, "spans ascend");
    }
}

#[test]
fn mixed_line_endings_extract_with_their_own_terminators() {
    let source = fixture("mixed-endings.adoc");
    let records = extract(&source).unwrap();
    assert_eq!(records.len(), 1, "mixed endings are supported");
    assert_eq!(records[0].id, "SWR-0320");
    assert_eq!(
        records[0].body_raw,
        "The system shall tolerate mixed line endings.\r\n"
    );
}

#[test]
fn unterminated_block_is_a_parse_error_naming_file_and_location() {
    for source in [
        fixture("unterminated.adoc"),
        fixture("unterminated-crlf.adoc"),
        "[requirement#SWR-0001]\n--\nbody\n\nand more prose\n".to_owned(),
        "[requirement#SWR-0001]\r\n--\r\nbody\r\n".to_owned(),
        "--\n".to_owned(),
    ] {
        let error = extract(&source).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("t.adoc"), "names the file: {message}");
        assert!(message.contains("line "), "gives the line: {message}");
        assert!(
            message.contains("byte offset"),
            "gives the offset: {message}"
        );
        assert!(message.contains("unterminated"), "{message}");
    }
}

#[test]
fn empty_block_is_rejected_as_unterminated_and_blank_body_is_valid() {
    // acdc reads `--` immediately followed by `--` as the start of a nested
    // open block, so an "empty" requirement never terminates.
    let error = extract(&fixture("empty-block.adoc")).unwrap_err();
    assert!(error.to_string().contains("unterminated"), "{error}");
    // A block whose body is one blank line does terminate and extracts.
    let records = extract("[requirement#SWR-0302]\n--\n\n--\n").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].body_raw, "\n");
}

#[test]
fn blocks_with_and_without_trailing_newline_extract_the_same_body() {
    let valid = fixture("valid-lf.adoc");
    let with = extract(&valid).unwrap();
    // A file ending right after the close delimiter; the source is derived
    // here because the pre-commit end-of-file fixer forbids such a fixture.
    let without_source = valid.trim_end_matches('\n');
    let without = extract(without_source).unwrap();
    assert_eq!(with.len(), 1);
    assert_eq!(without.len(), 1);
    assert_eq!(with[0].body_raw, "The system shall beep.\n");
    assert_eq!(without[0].body_raw, "The system shall beep.\n");
    assert_eq!(with[0].span.end, valid.len() - 1, "ends before the newline");
    assert_eq!(
        without[0].span.end,
        valid.len() - 1,
        "ends at the last byte of the close delimiter"
    );
}

#[test]
fn example_block_with_requirement_style_is_rejected() {
    for source in [
        fixture("example-requirement.adoc"),
        crlf(&fixture("example-requirement.adoc")),
    ] {
        let error = extract(&source).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("t.adoc"), "names the file: {message}");
        assert!(message.contains("===="), "names the delimiter: {message}");
        assert!(message.contains("line "), "gives the line: {message}");
        assert!(
            message.contains("only `--` open blocks"),
            "states the rule: {message}"
        );
    }
}

#[test]
fn indented_requirement_is_rejected() {
    for source in [
        fixture("indented.adoc"),
        crlf(&fixture("indented.adoc")),
        "[requirement#SWR-0001]\n[status=draft]\n\t--\nbody\n\t--\n".to_owned(),
        "[requirement#SWR-0001]\r\n  --\r\n  body\r\n  --\r\n".to_owned(),
    ] {
        let error = extract(&source).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("t.adoc"), "names the file: {message}");
        assert!(message.contains("indented"), "{message}");
        assert!(message.contains("line "), "gives the line: {message}");
    }
    // Attribute lines without a requirement style or id anchor are not
    // requirements, even above an indented `--`.
    let records = extract("[.note]\n  --\n  not a requirement\n  --\n").unwrap();
    assert!(records.is_empty());
}

#[test]
fn nested_example_inside_the_body_is_preserved() {
    let source = fixture("nested-example.adoc");
    let records = extract(&source).unwrap();
    assert_eq!(records.len(), 1);
    let expected = "\
The controller shall log nested examples.

.An example inside the requirement body
[.example]
====
An example nested inside a requirement.
====
";
    assert_eq!(records[0].body_raw, expected);
    assert!(records[0].statement().contains("shall"));
}

#[test]
fn a_large_body_is_preserved_byte_for_byte() {
    let lines: Vec<String> = (0..400).map(|i| format!("body line {i}")).collect();
    let source = format!(
        "[requirement#SWR-0009]\n\
         [modality=shall, kind=functional, verification=test]\n\
         --\n{}\n--\n",
        lines.join("\n")
    );
    let records = extract(&source).unwrap();
    assert_eq!(records.len(), 1);
    let body: Vec<String> = records[0].body_raw.lines().map(str::to_owned).collect();
    assert_eq!(body.len(), 400);
    assert_eq!(body[0], "body line 0");
    assert_eq!(body[399], "body line 399");
    assert_eq!(records[0].body_raw, lines.join("\n") + "\n");
}

/// Any input may be extracted without panicking; successful records always
/// describe substrings of the source with in-range spans.
#[test]
fn malformed_input_never_panics() {
    let cases = [
        "",
        "\n",
        "--",
        "--\n",
        "--\n--",
        "--\n--\n--\n--\n",
        "\r\n--\r\n--\r\n",
        "[",
        "[]",
        "[#]",
        "[a#b#c]",
        "[requirement#X]",
        "[requirement#X]\n--",
        "[requirement#X]\n--\n",
        "[requirement#X]\n====\n",
        "====\n====\n",
        "text\n--\n",
        "\r",
        "\r\n",
        "[requirement#X]\r[status=draft]\r--\r",
        "\0\n--\n\0\n--\n",
        "[\u{0}]\n--\n\u{1}\n--\n",
        "[requirement#X]\n--\n日本語 — ✨ body\n--\n",
        "[requirement#Ünïcode-✓]\n--\nbødy\n--\n",
        "[requirement#X]\n--\nbody\r--\n--\n",
    ];
    for source in cases {
        if let Ok(records) = extract(source) {
            for record in &records {
                assert_eq!(record.delim, "--");
                assert!(!record.id.is_empty());
                assert!(record.span.start <= record.span.end);
                assert!(record.span.end <= source.len(), "{record:?}");
                assert!(
                    source.contains(record.body_raw.as_str()),
                    "body_raw is a substring of the source: {record:?}"
                );
                for line in &record.attr_lines {
                    assert!(line.start <= line.end);
                    assert!(line.end <= source.len());
                    assert!(source.contains(line.raw.as_str()));
                }
            }
        }
    }
}

#[test]
fn lone_cr_is_one_line_with_no_requirements() {
    let records = extract(&fixture("lone-cr.adoc")).unwrap();
    assert!(records.is_empty(), "lone CR is not a line terminator");
}

#[test]
fn a_title_above_the_attributes_stays_outside_the_record() {
    let source = fixture("title.adoc");
    let records = extract(&source).unwrap();
    assert_eq!(records.len(), 1);
    let attr_start = source.find("[requirement#SWR-0307]").unwrap();
    assert_eq!(records[0].span.start, attr_start);
    assert_eq!(records[0].id, "SWR-0307");
    assert_eq!(
        records[0].statement(),
        "The system shall carry a title above its attribute lines."
    );
}

#[test]
fn check_treats_crlf_like_lf() {
    let disagreement = VALID.replace("shall beep", "may beep");
    for source in [VALID, &crlf(VALID)] {
        assert!(
            violations(source).is_empty(),
            "valid CRLF must not raise V4"
        );
    }
    for source in [disagreement.as_str(), crlf(&disagreement).as_str()] {
        assert_eq!(
            rules(&violations(source)),
            vec![vaire::check::ValidationRule::ModalityDisagreement],
            "a real disagreement still raises exactly V4"
        );
    }
}

#[test]
fn crlf_extract_emit_is_byte_identical_and_idempotent() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("crlf.adoc");
    fs::write(&path, crlf(VALID)).unwrap();
    let file = path.to_str().unwrap().to_owned();

    let extract_json = |path: &str| {
        let source = fs::read_to_string(path).unwrap();
        serde_json::to_string(&vaire::extract::extract(path, &source).unwrap()).unwrap()
    };

    // Unedited records leave the file byte-identical.
    let json = extract_json(&file);
    vaire::emit::emit(&json, &file).unwrap();
    assert_eq!(fs::read(&path).unwrap(), crlf(VALID).as_bytes());

    // An edited attribute line splices without touching any terminator.
    let mut records: Vec<vaire::Record> = serde_json::from_str(&json).unwrap();
    for item in &mut records[0].attr_lines[2].items {
        if let vaire::Item::Kv { name, value } = item
            && name == "status"
        {
            *value = "active".to_owned();
        }
    }
    let edited = serde_json::to_string(&records).unwrap();
    vaire::emit::emit(&edited, &file).unwrap();
    let expected = crlf(VALID).replace("[status=draft]", "[status=active]");
    assert_ne!(expected, crlf(VALID));
    assert_eq!(fs::read(&path).unwrap(), expected.as_bytes());
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("[status=active]\r\n"), "CRLF preserved");

    // Idempotence: a second extract-emit pass changes nothing.
    let json = extract_json(&file);
    vaire::emit::emit(&json, &file).unwrap();
    assert_eq!(fs::read(&path).unwrap(), expected.as_bytes());
}
