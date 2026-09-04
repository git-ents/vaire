//! Attribute-value quoting and escaping: the renderer and parser round-trip
//! values exactly, reject values with no single-line representation, and
//! keep legacy unescaped lines parsing unchanged.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]

use std::fs;
use std::path::PathBuf;

const SOURCE: &str =
    "[requirement#SWR-0001]\n[modality=shall, status=draft]\n--\nThe system shall beep.\n--\n";

/// Values the renderer and parser must round-trip byte-exactly.
const VALUES: &[&str] = &[
    "approved",
    "a,b",
    "a]b",
    "a]",
    "a\"b",
    "a\"b, c]",
    "C:\\path",
    "a\"b\\c",
    "",
    " padded ",
    "déjà vu \"✓\"",
];

fn temp(name: &str, contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("vaire-quote-{}-{name}", std::process::id()));
    fs::write(&path, contents).unwrap();
    path
}

fn extract(path: &std::path::Path) -> Vec<vaire::Record> {
    let file = path.to_str().unwrap().to_owned();
    let source = fs::read_to_string(path).unwrap();
    vaire::extract::extract(&file, &source).unwrap()
}

fn json(records: &[vaire::Record]) -> String {
    serde_json::to_string(records).unwrap()
}

fn set_status(records: &mut [vaire::Record], value: &str) {
    for record in records {
        for line in &mut record.attr_lines {
            for item in &mut line.items {
                if let vaire::Item::Kv { name, value: v } = item
                    && name == "status"
                {
                    *v = value.to_owned();
                }
            }
        }
    }
}

/// Fresh extraction with every `status` value set to `value`, as JSON.
fn edited_json(path: &std::path::Path, value: &str) -> String {
    let mut records = serde_json::from_str::<Vec<vaire::Record>>(&json(&extract(path))).unwrap();
    set_status(&mut records, value);
    json(&records)
}

#[test]
fn edited_values_round_trip_through_emit() {
    for value in VALUES {
        let path = temp("rt.adoc", SOURCE);
        let file = path.to_str().unwrap().to_owned();
        let edited = edited_json(&path, value);

        vaire::emit::emit(&edited, &file).unwrap();
        let after = extract(&path);
        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0].field("status").as_deref(),
            Some(*value),
            "value {value:?} did not survive emit"
        );

        // The bug's exact shape renders as valid, escape-carrying syntax.
        if *value == "a\"b, c]" {
            let written = fs::read_to_string(&path).unwrap();
            assert!(
                written.contains("[modality=shall, status=\"a\\\"b, c]\"]"),
                "got {written:?}"
            );
        }

        // Repeated emits of fresh extractions plan nothing and change
        // nothing; the pre-edit records are stale by span and rejected.
        let once = fs::read_to_string(&path).unwrap();
        vaire::emit::emit(&json(&extract(&path)), &file).unwrap();
        vaire::emit::emit(&json(&extract(&path)), &file).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), once, "not idempotent");

        fs::remove_file(&path).expect("cleanup temp file");
    }
}

#[test]
fn several_edited_attributes_round_trip_in_one_record() {
    let path = temp("multi.adoc", SOURCE);
    let file = path.to_str().unwrap().to_owned();
    let mut records = serde_json::from_str::<Vec<vaire::Record>>(&json(&extract(&path))).unwrap();
    for item in &mut records[0].attr_lines[1].items {
        if let vaire::Item::Kv { name, value } = item {
            match name.as_str() {
                "modality" => *value = "shall, unless overridden".to_owned(),
                "status" => *value = "in review".to_owned(),
                _ => {}
            }
        }
    }
    let edited = json(&records);

    vaire::emit::emit(&edited, &file).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("[modality=\"shall, unless overridden\", status=\"in review\"]"),
        "got {written:?}"
    );
    let after = extract(&path);
    assert_eq!(
        after[0].field("modality").as_deref(),
        Some("shall, unless overridden")
    );
    assert_eq!(after[0].field("status").as_deref(), Some("in review"));

    let once = fs::read_to_string(&path).unwrap();
    vaire::emit::emit(&json(&extract(&path)), &file).unwrap();
    vaire::emit::emit(&json(&extract(&path)), &file).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), once, "not idempotent");
    fs::remove_file(&path).expect("cleanup temp file");
}

#[test]
fn a_value_with_a_line_break_is_rejected_and_the_file_untouched() {
    let path = temp("nl.adoc", SOURCE);
    let file = path.to_str().unwrap().to_owned();
    for value in ["beep\nboop", "beep\rboop"] {
        let edited = edited_json(&path, value);
        let error = vaire::emit::emit(&edited, &file).unwrap_err();
        assert!(matches!(error, vaire::Error::Unrepresentable(_)), "{error}");
        let message = error.to_string();
        assert!(message.contains("line break"), "{message}");
        assert!(message.contains("single-line"), "{message}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            SOURCE,
            "rejected emit wrote the file"
        );
    }
    fs::remove_file(&path).expect("cleanup temp file");
}

#[test]
fn malformed_escapes_and_legacy_lines_have_defined_values() {
    let parse = |line: &str| {
        vaire::extract::parse_attr_line(line, 0, line.len())
            .items
            .pop()
            .unwrap()
    };
    // `\"` at the end never closes the quote: the escape decodes and the
    // trailing backslash stays, instead of rejecting a hand-edited line.
    assert_eq!(
        parse("[status=\"a\\\"]"),
        vaire::Item::Kv {
            name: "status".to_owned(),
            value: "a\\".to_owned()
        }
    );
    // An unterminated quote passes through literally, as it always has.
    assert_eq!(
        parse("[status=\"abc]"),
        vaire::Item::Kv {
            name: "status".to_owned(),
            value: "\"abc".to_owned()
        }
    );
    // Legacy unescaped lines: quotes without backslashes and bare
    // backslashes outside quotes keep their old values.
    assert_eq!(
        parse("[rationale=\"safety interlock\"]"),
        vaire::Item::Kv {
            name: "rationale".to_owned(),
            value: "safety interlock".to_owned()
        }
    );
    assert_eq!(
        parse("[source=C:\\path]"),
        vaire::Item::Kv {
            name: "source".to_owned(),
            value: "C:\\path".to_owned()
        }
    );
}

#[test]
fn render_and_parse_are_stable_for_tricky_values() {
    for value in VALUES {
        let items = vec![vaire::Item::Kv {
            name: "status".to_owned(),
            value: (*value).to_owned(),
        }];
        let first = vaire::extract::render_attr_line(&items).unwrap();
        let reparsed = vaire::extract::parse_attr_line(&first, 0, first.len());
        assert_eq!(reparsed.items, items, "value {value:?} did not re-parse");
        let second = vaire::extract::render_attr_line(&reparsed.items).unwrap();
        assert_eq!(second, first, "value {value:?} is not render-stable");
        let again = vaire::extract::parse_attr_line(&second, 0, second.len());
        assert_eq!(again.items, reparsed.items, "value {value:?} drifted");
    }
}

#[test]
fn corpus_attribute_lines_render_back_to_their_raw_bytes() {
    for name in ["seed.adoc", "misc.adoc", "traces.adoc", "crlf.adoc"] {
        let source = fs::read_to_string(format!("tests/corpus/{name}")).unwrap();
        for record in vaire::extract::extract(name, &source).unwrap() {
            assert!(!record.attr_lines.is_empty());
            for line in record.attr_lines {
                let rendered = vaire::extract::render_attr_line(&line.items).unwrap();
                assert_eq!(
                    rendered, line.raw,
                    "{name}: record {}: canonical rendering must reproduce the source line",
                    record.id
                );
            }
        }
    }
}
