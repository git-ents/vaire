#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use unwraps, direct indexing, and panics to express failed expectations"
)]
use std::fs;
use std::path::PathBuf;

fn temp(scope: &str, name: &str, contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("vaire-check-{scope}-{}-{name}", std::process::id()));
    fs::write(&path, contents).unwrap();
    path
}

fn validate_src(scope: &str, name: &str, src: &str) -> Vec<vaire::check::Violation> {
    let path = temp(scope, name, src);
    let file = path.to_str().unwrap().to_owned();
    let records = vaire::extract(&file, src).unwrap();
    let v = vaire::check::validate(&records);
    fs::remove_file(&path).expect("cleanup temp file");
    v
}

fn rules(v: &[vaire::check::Violation]) -> Vec<vaire::check::ValidationRule> {
    v.iter().map(|x| x.rule).collect()
}

/// One minimal failing document per rule, straight from the spec's examples
/// (`tests/invalid/`): the file must fail with that rule and that rule only.
#[test]
fn each_spec_invalid_example_fails_its_rule() {
    for (rule, name) in [
        (
            vaire::check::ValidationRule::DuplicateId,
            "v1-duplicate-id.adoc",
        ),
        (
            vaire::check::ValidationRule::UnresolvedTrace,
            "v2-unresolved-trace.adoc",
        ),
        (
            vaire::check::ValidationRule::MissingVerification,
            "v3-missing-verification.adoc",
        ),
        (
            vaire::check::ValidationRule::ModalityDisagreement,
            "v4-modality-disagreement.adoc",
        ),
        (
            vaire::check::ValidationRule::CompoundStatement,
            "v5-compound-statement.adoc",
        ),
        (
            vaire::check::ValidationRule::UnknownAttribute,
            "v6-unknown-attribute.adoc",
        ),
    ] {
        let src = fs::read_to_string(format!("tests/invalid/{name}")).unwrap();
        let violations = validate_src("ex", name, &src);
        assert_eq!(rules(&violations), vec![rule], "{name}: {violations:?}");
    }
}

#[test]
fn clean_corpus_has_no_violations() {
    for name in ["seed.adoc", "traces.adoc", "crlf.adoc"] {
        let src = fs::read_to_string(format!("tests/corpus/{name}")).unwrap();
        let path = temp("clean", name, &src);
        let file = path.to_str().unwrap().to_owned();
        let violations = vaire::check::check(&[file]).unwrap();
        assert!(violations.is_empty(), "{name}: {violations:?}");
        fs::remove_file(&path).expect("cleanup temp file");
    }
}

/// Two records; the second (starting on line 7) disagrees on modality.
const TWO_RECORDS: &str = "[requirement#SWR-0001]\n[modality=shall, kind=functional, verification=test]\n--\nThe system shall boot.\n--\n\n[requirement#SWR-0002]\n[modality=shall, kind=functional, verification=test]\n--\nThe system may lag.\n--\n";

#[test]
fn violations_carry_offsets_and_source_lines() {
    let path = temp("loc", "two.adoc", TWO_RECORDS);
    let file = path.to_str().unwrap().to_owned();
    let violations = vaire::check::check(&[file]).unwrap();
    assert_eq!(
        rules(&violations),
        vec![vaire::check::ValidationRule::ModalityDisagreement]
    );
    assert_eq!(violations[0].line, Some(7));
    assert_eq!(
        violations[0].offset,
        TWO_RECORDS.find("[requirement#SWR-0002]").unwrap()
    );
    fs::remove_file(&path).expect("cleanup temp file");
}

#[test]
fn crlf_sources_keep_line_numbers_and_raw_offsets() {
    let crlf = TWO_RECORDS.replace('\n', "\r\n");
    let path = temp("loc", "two-crlf.adoc", &crlf);
    let file = path.to_str().unwrap().to_owned();
    let violations = vaire::check::check(&[file]).unwrap();
    assert_eq!(violations[0].line, Some(7));
    let offset = TWO_RECORDS.find("[requirement#SWR-0002]").unwrap();
    let terminators = TWO_RECORDS.get(..offset).unwrap().matches('\n').count();
    assert_eq!(violations[0].offset, offset + terminators);
    fs::remove_file(&path).expect("cleanup temp file");
}

#[test]
fn validate_without_sources_leaves_lines_unset() {
    let records = vaire::extract("two.adoc", TWO_RECORDS).unwrap();
    let violations = vaire::check::validate(&records);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].line.is_none());
    assert_eq!(violations[0].offset, records[1].span.start);
}

#[test]
fn keyword_matching_is_case_insensitive_and_punctuation_tolerant() {
    for (name, src) in [
        (
            "prose-uppercase.adoc",
            "[requirement#SWR-0110]\n[modality=shall, kind=functional, verification=test]\n--\nThe system SHALL retry, every time.\n--\n",
        ),
        (
            "attribute-uppercase.adoc",
            "[requirement#SWR-0111]\n[modality=SHALL, kind=functional, verification=test]\n--\nThe system shall retry, every time.\n--\n",
        ),
    ] {
        let violations = validate_src("kw", name, src);
        assert!(violations.is_empty(), "{name}: {violations:?}");
    }
}

#[test]
fn multiline_statements_carry_the_keyword_on_any_line() {
    let src = "[requirement#SWR-0112]\n[modality=shall, kind=functional, verification=test]\n--\nThe logging subsystem\nshall persist faults across restarts.\n--\n";
    let violations = validate_src("kw", "multiline.adoc", src);
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn a_quoted_keyword_counts_toward_compound() {
    let src = "[requirement#SWR-0106]\n[modality=shall, kind=functional, verification=test]\n--\nThe system shall log the literal `shall` verbatim.\n--\n";
    let violations = validate_src("kw", "quoted.adoc", src);
    assert_eq!(
        rules(&violations),
        vec![vaire::check::ValidationRule::CompoundStatement]
    );
}
