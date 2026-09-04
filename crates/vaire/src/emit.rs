//! Surgical emit: splice edited attribute lines into the original bytes.

use std::fs;

use crate::extract::{extract, render_attr_line};
use crate::{Error, Record, Result};

/// One attribute-line rewrite planned against the live source.
///
/// `old`/`new` are the exact byte regions in the current file, so the plan is
/// renderable as a diff without touching disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub id: String,
    pub start: usize,
    pub end: usize,
    pub old: String,
    pub new: String,
}

/// Compute the splices `emit` would apply to `path` for the records in `json`.
///
/// Only attribute lines whose items differ from the current source are
/// planned; body bytes, delimiters, and all other content are untouched.
pub fn plan(json: &str, path: &str) -> Result<Vec<Edit>> {
    let records: Vec<Record> = serde_json::from_str(json)?;
    let source = fs::read_to_string(path)?;
    let current = extract(path, &source)?;
    let by_id: std::collections::HashMap<&str, &Record> =
        current.iter().map(|r| (r.id.as_str(), r)).collect();

    let mut edits: Vec<Edit> = Vec::new();
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
            if want.items != have.items {
                edits.push(Edit {
                    id: record.id.clone(),
                    start: have.start,
                    end: have.end,
                    old: have.raw.clone(),
                    new: render_attr_line(&want.items),
                });
            }
        }
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.start));
    Ok(edits)
}

/// Apply the records in `json` (from a prior `extract`) to `path`, in place.
///
/// Writes only if the plan is nonempty; an empty plan leaves the file and its
/// mtime untouched.
pub fn emit(json: &str, path: &str) -> Result<()> {
    let edits = plan(json, path)?;
    if edits.is_empty() {
        return Ok(());
    }
    let mut source = fs::read_to_string(path)?;
    for edit in &edits {
        source.replace_range(edit.start..edit.end, &edit.new);
    }
    fs::write(path, source)?;
    Ok(())
}

/// Convenience for tests and tools: parse a JSON record set.
pub fn records_from_json(json: &str) -> Result<Vec<Record>> {
    serde_json::from_str(json).map_err(Error::Json)
}
