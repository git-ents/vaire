#![expect(
    clippy::unwrap_used,
    reason = "tests unwrap deliberately; a panic fails the test"
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
