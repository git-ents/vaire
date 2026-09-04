#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests unwrap deliberately and index only into bounds they just built; a panic fails the test"
)]

use std::fs;
use std::path::PathBuf;

/// Deterministic LCG so the property test is reproducible without a rand dep.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn temp(name: &str, contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("vaire-prop-{}-{name}", std::process::id()));
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn random_field_edits_change_only_their_lines() {
    let original = fs::read_to_string("tests/corpus/traces.adoc").expect("corpus file");
    let mut rng = Lcg(0x5EED_CAFE);
    let statuses = ["approved", "draft", "rejected"];
    let volatilities = ["low", "high"];

    for trial in 0..64 {
        let path = temp(&format!("p{trial}.adoc"), &original);
        let file = path.to_str().unwrap().to_owned();

        // Pick a random record and a random editable field on it.
        let src = fs::read_to_string(&path).unwrap();
        let records = vaire::extract(&file, &src).unwrap();
        let record = &records[rng.next() as usize % records.len()];
        let fields: Vec<(String, String)> = record
            .fields()
            .into_iter()
            .filter(|(n, _)| n == "status" || n == "volatility" || n == "modality")
            .collect();
        if fields.is_empty() {
            fs::remove_file(&path).expect("cleanup temp file");
            continue;
        }
        let (key, old) = &fields[rng.next() as usize % fields.len()];
        let new_value = match key.as_str() {
            "status" => statuses[rng.next() as usize % statuses.len()].to_owned(),
            "volatility" => volatilities[rng.next() as usize % volatilities.len()].to_owned(),
            _ => ["shall", "should", "may"][rng.next() as usize % 3].to_owned(),
        };
        if &new_value == old {
            fs::remove_file(&path).expect("cleanup temp file");
            continue;
        }

        // Apply the edit through the JSON contract.
        let mut json: Vec<vaire::Record> =
            serde_json::from_str(&serde_json::to_string(&records).unwrap()).unwrap();
        let target = json.iter_mut().find(|r| r.id == record.id).unwrap();
        let mut edited_line_index = None;
        for (i, line) in target.attr_lines.iter_mut().enumerate() {
            for item in &mut line.items {
                if let vaire::Item::Kv { name, value } = item
                    && name == key
                {
                    *value = new_value.clone();
                    edited_line_index = Some(i);
                }
            }
        }
        assert!(edited_line_index.is_some(), "field lives on an attr line");
        let json = serde_json::to_string(&json).unwrap();
        vaire::emit::emit(&json, &file).unwrap();

        // Diff: exactly one line changed, and it is the line that field lives on.
        let after = fs::read_to_string(&path).unwrap();
        let before_lines: Vec<&str> = original.lines().collect();
        let after_lines: Vec<&str> = after.lines().collect();
        assert_eq!(before_lines.len(), after_lines.len(), "line count drifted");
        let changed: Vec<usize> = before_lines
            .iter()
            .zip(&after_lines)
            .enumerate()
            .filter_map(|(i, (a, b))| (a != b).then_some(i))
            .collect();
        assert_eq!(changed.len(), 1, "trial {trial}: expected one changed line");
        let changed_line = before_lines[changed[0]];
        assert!(
            changed_line.starts_with('[') && changed_line.contains(key),
            "trial {trial}: changed line `{changed_line}` is not the `{key}` attribute line"
        );
        assert!(
            after_lines[changed[0]].contains(&new_value),
            "trial {trial}: new value missing"
        );

        // Round-trip still holds after the edit.
        let json3 = {
            let src = fs::read_to_string(&path).unwrap();
            let records = vaire::extract(&file, &src).unwrap();
            serde_json::to_string(&records).unwrap()
        };
        vaire::emit::emit(&json3, &file).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), after, "not idempotent");

        fs::remove_file(&path).expect("cleanup temp file");
    }
}
