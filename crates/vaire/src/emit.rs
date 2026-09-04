//! Surgical emit: splice edited attribute lines into the original bytes.

use std::fs;

use crate::extract::{extract, render_attr_line};
use crate::{Error, Record, Result};

/// Apply the records in `json` (from a prior `extract`) to `path`, in place.
///
/// Only attribute lines whose items differ from the current source are
/// rewritten; body bytes, delimiters, and all other content are untouched.
pub fn emit(json: &str, path: &str) -> Result<()> {
    let records: Vec<Record> = serde_json::from_str(json)?;
    let source = fs::read_to_string(path)?;
    let current = extract(path, &source)?;
    let by_id: std::collections::HashMap<&str, &Record> =
        current.iter().map(|r| (r.id.as_str(), r)).collect();

    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    for record in records.iter().filter(|r| r.file == path) {
        let live = by_id.get(record.id.as_str()).ok_or_else(|| {
            Error::Unrepresentable(format!(
                "{path}: record {} not found in file (ids are immutable)",
                record.id
            ))
        })?;
        if record.attr_lines.len() != live.attr_lines.len() {
            return Err(Error::Unrepresentable(format!(
                "{path}: record {}: attribute line count changed",
                record.id
            )));
        }
        for (want, have) in record.attr_lines.iter().zip(&live.attr_lines) {
            if !items_equal(&want.items, &have.items) {
                replacements.push((have.start, have.end, render_attr_line(&want.items)));
            }
        }
    }

    replacements.sort_by_key(|&(start, _, _)| std::cmp::Reverse(start));
    let mut out = source.clone();
    for (start, end, text) in replacements {
        out.replace_range(start..end, &text);
    }
    if out != source {
        fs::write(path, out)?;
    }
    Ok(())
}

fn items_equal(a: &[crate::Item], b: &[crate::Item]) -> bool {
    a == b
}

/// Convenience for tests and tools: parse a JSON record set.
pub fn records_from_json(json: &str) -> Result<Vec<Record>> {
    serde_json::from_str(json).map_err(Error::Json)
}
