//! Direct attribute editing: set named attributes on one located requirement.
//!
//! Every edit reuses emit's machinery end to end: the live file is extracted,
//! the one requirement named by id is located, the requested sets overwrite
//! attribute values in that record, and the resulting record set runs through
//! [`crate::emit::plan`] and [`crate::emit::emit`] — so path identity,
//! stale-source verification, and the atomic temp+rename write are emit's
//! exactly. A set can only overwrite the value of a `Kv` item the record
//! already carries, so an edit can never add, remove, or restructure an
//! attribute line, never touches id or style items, and never touches the
//! body. Every attribute line carrying an edited key is rewritten, so no
//! shadowed stale value survives and the merged view (`Record::fields`) and
//! every raw line agree.

use std::fs;

use crate::emit::{self, Edit};
use crate::extract::extract;
use crate::show::location;
use crate::{Error, Item, KNOWN_KEYS, Record, Result};

/// One `KEY=VALUE` attribute assignment for [`plan`] and [`edit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set {
    /// The attribute name; the target record must already carry it.
    pub key: String,
    /// The replacement value, verbatim; rendering follows the attribute-line
    /// quoting contract, so quotes, commas, brackets, and whitespace are
    /// escaped as needed.
    pub value: String,
}

impl Set {
    /// Parse one `KEY=VALUE` argument: the first `=` splits, so the value may
    /// contain further `=` characters.
    pub fn parse(raw: &str) -> Result<Self> {
        let Some((key, value)) = raw.split_once('=') else {
            return Err(Error::Validation(format!(
                "expected KEY=VALUE, got `{raw}`"
            )));
        };
        if key.is_empty() {
            return Err(Error::Validation(format!(
                "empty attribute name in `{raw}`"
            )));
        }
        Ok(Self {
            key: key.to_owned(),
            value: value.to_owned(),
        })
    }
}

/// Compute the splices setting `sets` on requirement `id` would apply to
/// `file`, without writing anything.
///
/// `id` must match exactly one requirement in the file, the file must extract
/// cleanly, and every key must be in the vaire vocabulary and already present
/// on that record as a named attribute. All checks run before any splice is
/// computed, so a rejected edit plans nothing.
pub fn plan(file: &str, id: &str, sets: &[Set]) -> Result<Vec<Edit>> {
    let json = edited_records_json(file, id, sets)?;
    emit::plan(&json, file)
}

/// Set `sets` on requirement `id` in `file`, in place, atomically.
///
/// All of [`plan`]'s preconditions apply; on success every attribute line
/// carrying an edited key has been rewritten with the new value and all other
/// bytes are untouched. An edit whose values already match the file plans no
/// splices and writes nothing, leaving the bytes and mtime alone.
pub fn edit(file: &str, id: &str, sets: &[Set]) -> Result<()> {
    let json = edited_records_json(file, id, sets)?;
    emit::emit(&json, file)
}

/// The record set `emit` would write for this edit: the live extraction with
/// `sets` applied to the one record named `id`.
fn edited_records_json(file: &str, id: &str, sets: &[Set]) -> Result<String> {
    let source = fs::read_to_string(file).map_err(|e| Error::Missing(format!("{file}: {e}")))?;
    let records = extract(file, &source)?;
    validate_sets(locate(file, &source, &records, id)?, sets)?;
    let mut records = records;
    for record in records.iter_mut().filter(|r| r.id == id) {
        apply(record, sets);
    }
    serde_json::to_string(&records).map_err(Error::Json)
}

/// The one record named `id`, or an error naming the file, the id, and for a
/// duplicate every occurrence's location.
fn locate<'a>(file: &str, source: &str, records: &'a [Record], id: &str) -> Result<&'a Record> {
    let hits: Vec<&Record> = records.iter().filter(|r| r.id == id).collect();
    match hits.as_slice() {
        [] => Err(Error::Missing(format!(
            "{file}: requirement `{id}` not found"
        ))),
        [record] => Ok(record),
        dups => Err(Error::Missing(format!(
            "{file}: requirement id `{id}` appears {} times: {}",
            dups.len(),
            dups.iter()
                .map(|r| location(source, r.span.start))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Reject anything an edit cannot express as a value overwrite on existing
/// attribute lines: repeated keys, structural names, out-of-vocabulary keys,
/// and keys the record does not carry.
fn validate_sets(record: &Record, sets: &[Set]) -> Result<()> {
    for (i, set) in sets.iter().enumerate() {
        if sets.iter().take(i).any(|s| s.key == set.key) {
            return Err(Error::Validation(format!(
                "`{}` is set more than once; set each attribute at most once per edit",
                set.key
            )));
        }
        if let Some(reason) = structural_reason(&set.key) {
            return Err(Error::Validation(format!(
                "cannot set `{}`: {reason}",
                set.key
            )));
        }
        if !KNOWN_KEYS.contains(&set.key.as_str()) {
            return Err(Error::Validation(format!(
                "`{}` is not in the vaire attribute vocabulary (check rule V6)",
                set.key
            )));
        }
        if !carries(record, &set.key) {
            let present = record
                .fields()
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let known = if present.is_empty() {
                String::new()
            } else {
                format!("; it carries: {present}")
            };
            return Err(Error::Validation(format!(
                "record {} carries no attribute `{}`{known}; edit only overwrites \
                 attributes already present",
                record.id, set.key
            )));
        }
    }
    Ok(())
}

/// Why a structural name cannot be set: id and style belong to the positional
/// grammar, and the body is not an attribute at all.
fn structural_reason(key: &str) -> Option<&'static str> {
    match key {
        "id" => Some("ids are immutable"),
        "style" => Some("style is positional grammar, not a named attribute"),
        "body" => Some("bodies are not editable via edit"),
        _ => None,
    }
}

/// Whether any attribute line of `record` carries `key` as a named attribute.
fn carries(record: &Record, key: &str) -> bool {
    record.attr_lines.iter().any(|line| {
        line.items
            .iter()
            .any(|item| matches!(item, Item::Kv { name, .. } if name == key))
    })
}

/// Overwrite the value of every `Kv` item named by a set; validation has
/// already proven each key is carried somewhere in the record.
fn apply(record: &mut Record, sets: &[Set]) {
    for line in &mut record.attr_lines {
        for item in &mut line.items {
            if let Item::Kv { name, value } = item
                && let Some(set) = sets.iter().find(|s| s.key == *name)
            {
                *value = set.value.clone();
            }
        }
    }
}
