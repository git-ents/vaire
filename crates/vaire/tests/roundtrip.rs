#![expect(
    clippy::unwrap_used,
    reason = "tests unwrap deliberately and index only into bounds they just built; a panic fails the test"
)]

use std::fs;
use std::path::PathBuf;

fn corpus_file(name: &str) -> String {
    fs::read_to_string(format!("tests/corpus/{name}")).unwrap()
}

fn temp_copy(scope: &str, name: &str, contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("vaire-{scope}-{}-{name}", std::process::id()));
    fs::write(&path, contents).unwrap();
    path
}

fn extract_json(path: &std::path::Path) -> String {
    let file = path.to_str().unwrap().to_owned();
    let src = fs::read_to_string(path).unwrap();
    let records = vaire::extract(&file, &src).unwrap();
    serde_json::to_string(&records).unwrap()
}

#[test]
fn round_trip_is_byte_identical() {
    for name in ["seed.adoc", "misc.adoc", "traces.adoc", "crlf.adoc"] {
        let original = corpus_file(name);
        let path = temp_copy("rt", name, &original);
        let file = path.to_str().unwrap().to_owned();

        let json = extract_json(&path);
        vaire::emit::emit(&json, &file).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            original,
            "emit(extract(x)) != x for {name}"
        );

        // Idempotence: emit(extract(emit(extract(x)))) == emit(extract(x)).
        let after_first = fs::read_to_string(&path).unwrap();
        let json2 = extract_json(&path);
        vaire::emit::emit(&json2, &file).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            after_first,
            "second emit changed the file for {name}"
        );

        fs::remove_file(&path).expect("cleanup temp file");
    }
}

#[test]
fn records_round_trip_through_json() {
    for name in ["seed.adoc", "misc.adoc", "traces.adoc", "crlf.adoc"] {
        let original = corpus_file(name);
        let path = temp_copy("json", name, &original);
        let file = path.to_str().unwrap().to_owned();
        let json = extract_json(&path);
        let records: Vec<vaire::Record> = serde_json::from_str(&json).unwrap();
        assert!(!records.is_empty(), "{name} produced no records");
        for r in &records {
            assert_eq!(r.file, file);
            assert!(r.id.starts_with("SWR") || r.id.starts_with("SYR"));
        }
        fs::remove_file(&path).expect("cleanup temp file");
    }
}
