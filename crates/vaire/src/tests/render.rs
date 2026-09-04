//! Rendering tests with forced color choices.

#![expect(
    clippy::unwrap_used,
    reason = "tests use unwraps to express failed expectations"
)]

use anstream::ColorChoice;

use super::{diff, table, violation_line};
use vaire::check::{ValidationRule, Violation};
use vaire::emit::Edit;
use vaire::extract::extract;

const SOURCE: &str = "[requirement#SWR-0001]\n[kind=functional]\n[modality=shall]\n[status=draft]\n[verification=test]\n--\nThe system *shall* record each requirement.\n--\n";

#[test]
fn violation_line_without_color_has_no_escapes() {
    let rendered = render(|out| violation_line(out, &violation(), ColorChoice::Never));
    assert_eq!(
        rendered,
        "V4: seed.adoc: modality disagrees: prose says must [SWR-0001]\n"
    );
}

#[test]
fn violation_line_with_color_is_styled() {
    let rendered = render(|out| violation_line(out, &violation(), ColorChoice::AlwaysAnsi));
    assert!(
        rendered.contains("\x1b[1m\x1b[31mV4\x1b[0m"),
        "got {rendered:?}"
    );
}

#[test]
fn table_lists_fields_and_statement() {
    let records = extract("seed.adoc", SOURCE).unwrap();
    let rendered = render(|out| table(out, "seed.adoc", &records, ColorChoice::Never));
    assert!(rendered.starts_with("--- seed.adoc\n"), "got {rendered:?}");
    assert!(
        rendered.contains("SWR-0001  functional  shall  draft  test"),
        "got {rendered:?}"
    );
    assert!(
        rendered.contains("The system *shall* record each requirement."),
        "got {rendered:?}"
    );
}

#[test]
fn table_colors_modality_by_strength() {
    let records = extract("seed.adoc", SOURCE).unwrap();
    let rendered = render(|out| table(out, "seed.adoc", &records, ColorChoice::AlwaysAnsi));
    assert!(
        rendered.contains("\x1b[1m\x1b[31mshall\x1b[0m"),
        "got {rendered:?}"
    );
}

#[test]
fn diff_shows_minus_and_plus_lines() {
    let edits = vec![Edit {
        id: "SWR-0001".to_owned(),
        start: 0,
        end: 10,
        old: "[status=draft]".to_owned(),
        new: "[status=active]".to_owned(),
    }];
    let rendered = render(|out| diff(out, "seed.adoc", &edits, ColorChoice::Never));
    assert!(rendered.contains("--- seed.adoc"), "got {rendered:?}");
    assert!(rendered.contains("@ SWR-0001"), "got {rendered:?}");
    assert!(rendered.contains("- [status=draft]"), "got {rendered:?}");
    assert!(rendered.contains("+ [status=active]"), "got {rendered:?}");
}

fn violation() -> Violation {
    Violation {
        rule: ValidationRule::ModalityDisagreement,
        file: "seed.adoc".to_owned(),
        id: "SWR-0001".to_owned(),
        message: "modality disagrees: prose says must".to_owned(),
    }
}

fn render(f: impl FnOnce(&mut Vec<u8>) -> std::io::Result<()>) -> String {
    let mut out = Vec::new();
    f(&mut out).unwrap();
    String::from_utf8(out).unwrap()
}
