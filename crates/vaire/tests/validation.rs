#![expect(
    clippy::unwrap_used,
    reason = "tests unwrap deliberately and index only into bounds they just built; a panic fails the test"
)]

use std::fs;
use std::path::PathBuf;

fn temp(name: &str, contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("vaire-check-{}-{name}", std::process::id()));
    fs::write(&path, contents).unwrap();
    path
}

fn validate_src(name: &str, src: &str) -> Vec<vaire::check::Violation> {
    let path = temp(name, src);
    let file = path.to_str().unwrap().to_owned();
    let records = vaire::extract(&file, src).unwrap();
    let v = vaire::check::validate(&records);
    fs::remove_file(&path).expect("cleanup temp file");
    v
}

fn rules(v: &[vaire::check::Violation]) -> Vec<vaire::check::ValidationRule> {
    v.iter().map(|x| x.rule).collect()
}

#[test]
fn v1_duplicate_id() {
    let src = r#"
[requirement#SWR-0100]
[modality=shall, kind=functional, verification=test]
--
The system shall log all faults.
--

[requirement#SWR-0100]
[modality=shall, kind=functional, verification=test]
--
The system shall rate-limit logs.
--
"#;
    let v = validate_src("v1.adoc", src);
    assert!(
        rules(&v).contains(&vaire::check::ValidationRule::DuplicateId),
        "{v:?}"
    );
}

#[test]
fn v2_unresolved_trace() {
    let src = r#"
[requirement#SWR-0101]
[modality=shall, kind=functional, verification=test]
[refines=SWR-9999]
--
The system shall persist faults across restarts.
--
"#;
    let v = validate_src("v2.adoc", src);
    assert!(
        rules(&v).contains(&vaire::check::ValidationRule::UnresolvedTrace),
        "{v:?}"
    );
}

#[test]
fn v3_missing_verification_on_leaf() {
    let src = r#"
[requirement#SWR-0102]
[modality=shall, kind=functional]
--
The system shall time-stamp every fault.
--
"#;
    let v = validate_src("v3.adoc", src);
    assert!(
        rules(&v).contains(&vaire::check::ValidationRule::MissingVerification),
        "{v:?}"
    );
}

#[test]
fn v4_modality_disagreement() {
    let src = r#"
[requirement#SWR-0103]
[modality=shall, kind=functional, verification=test]
--
The system may retry dropped samples.
--
"#;
    let v = validate_src("v4.adoc", src);
    assert!(
        rules(&v).contains(&vaire::check::ValidationRule::ModalityDisagreement),
        "{v:?}"
    );
}

#[test]
fn v5_compound_statement() {
    let src = r#"
[requirement#SWR-0104]
[modality=shall, kind=functional, verification=test]
--
The system shall retry a dropped sample and shall cap retries at two.
--
"#;
    let v = validate_src("v5.adoc", src);
    assert!(
        rules(&v).contains(&vaire::check::ValidationRule::CompoundStatement),
        "{v:?}"
    );
}

#[test]
fn v6_unknown_attribute() {
    let src = r#"
[requirement#SWR-0105]
[modality=shall, kind=functional, verification=test]
[priority=high]
--
The system shall degrade gracefully under load.
--
"#;
    let v = validate_src("v6.adoc", src);
    assert!(
        rules(&v).contains(&vaire::check::ValidationRule::UnknownAttribute),
        "{v:?}"
    );
}

#[test]
fn clean_corpus_has_no_violations() {
    for name in ["seed.adoc", "traces.adoc"] {
        let src = fs::read_to_string(format!("tests/corpus/{name}")).unwrap();
        let path = temp(name, &src);
        let file = path.to_str().unwrap().to_owned();
        let violations = vaire::check::check(&[file]).unwrap();
        assert!(violations.is_empty(), "{name}: {violations:?}");
        fs::remove_file(&path).expect("cleanup temp file");
    }
}
