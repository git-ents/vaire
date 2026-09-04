//! Surgical emit: splice edited attribute lines into the original bytes.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::extract::{extract, render_attr_line};
use crate::{AttrLine, Error, Item, Record, Result};

/// Monotonic suffix for temp files; with the pid this avoids colliding with
/// live temps, and `create_new` turns a stale same-name temp into an error.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

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
/// Computing a plan never writes anything.
///
/// # Path identity
///
/// A record applies to `path` only if its `file` field and `path` resolve to
/// the same file under [`std::fs::canonicalize`]: both sides are made
/// absolute, `.`/`..` are collapsed, and symlinks are resolved. Relative
/// paths, `./` prefixes, and symlink aliases of the target therefore all
/// match. Any record extracted from a different file is a hard error naming
/// both paths, as is a record whose `file` cannot be canonicalized (typically
/// extracted from a path that no longer exists). Nothing is filtered out
/// silently: mixed input is rejected on the first foreign record.
///
/// # Rejection semantics
///
/// Planning is all-or-nothing: the first inconsistency aborts with an error
/// and no edits are returned. Every supplied record must still describe the
/// live file (records in the file that the JSON does not mention are simply
/// left untouched). A plan is rejected when a record is supplied twice, its
/// id is absent from the file (ids are immutable), its byte span changed
/// (spans are bookkeeping and never editable), its block delimiter or
/// `body_raw` changed, its attribute-line count changed, its id attribute
/// item was rewritten, or an attribute line it wants to rewrite no longer
/// holds the bytes its `raw` carried at extraction time — refresh the JSON
/// with `vaire extract` against the file being edited.
pub fn plan(json: &str, path: &str) -> Result<Vec<Edit>> {
    let records: Vec<Record> = serde_json::from_str(json)?;
    let target = fs::canonicalize(path)?;
    require_same_file(&records, path, &target)?;
    let source = fs::read_to_string(path)?;
    let current = extract(path, &source)?;
    let by_id: HashMap<&str, &Record> = current.iter().map(|r| (r.id.as_str(), r)).collect();

    let mut supplied: HashSet<&str> = HashSet::new();
    let mut edits: Vec<Edit> = Vec::new();
    for record in &records {
        if !supplied.insert(record.id.as_str()) {
            return Err(Error::Unrepresentable(format!(
                "{path}: record {} supplied more than once",
                record.id
            )));
        }
        let live = by_id.get(record.id.as_str()).ok_or_else(|| {
            Error::Unrepresentable(format!(
                "{path}: record {} not found in file (ids are immutable)",
                record.id
            ))
        })?;
        if record.span != live.span {
            return Err(Error::Unrepresentable(format!(
                "{path}: record {}: byte span changed since extraction \
                 (spans are bookkeeping, not editable); re-extract",
                record.id
            )));
        }
        if record.delim != live.delim {
            return Err(Error::Unrepresentable(format!(
                "{path}: record {}: block delimiter changed since extraction \
                 (delimiters are not editable); re-extract",
                record.id
            )));
        }
        if record.body_raw != live.body_raw {
            return Err(Error::Unrepresentable(format!(
                "{path}: record {}: body changed since extraction \
                 (bodies are not editable); re-extract",
                record.id
            )));
        }
        if record.attr_lines.len() != live.attr_lines.len() {
            return Err(Error::Unrepresentable(format!(
                "{path}: record {}: attribute line count changed",
                record.id
            )));
        }
        for (want, have) in record.attr_lines.iter().zip(&live.attr_lines) {
            if id_items(want) != id_items(have) {
                return Err(Error::Unrepresentable(format!(
                    "{path}: record {}: id attribute item changed (ids are immutable)",
                    record.id
                )));
            }
            if want.items != have.items {
                if want.raw != have.raw {
                    return Err(Error::Unrepresentable(format!(
                        "{path}: record {}: attribute line changed since \
                         extraction (extracted {raw:?}, file now {live:?}); \
                         re-extract",
                        record.id,
                        raw = want.raw,
                        live = have.raw
                    )));
                }
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
/// mtime untouched. All of [`plan`]'s path-identity and rejection semantics
/// apply.
///
/// # Stale-source protection
///
/// Immediately before writing, the target is re-read and every planned edit
/// region must still contain the bytes the plan was computed from; anything
/// else aborts the whole operation with an error advising a re-extract, and
/// nothing is written. Combined with `plan`'s extraction-time checks, emit
/// never rewrites a region that changed after the records were made.
///
/// The replacement itself is atomic: the new bytes go to a temp file in the
/// target's directory, are synced, and are renamed over the target, so a
/// failure never leaves a partially applied or truncated file and readers
/// see either the old or the complete new content. The rename lands on the
/// canonical target, so a symlink alias stays a symlink to the updated file.
pub fn emit(json: &str, path: &str) -> Result<()> {
    let edits = plan(json, path)?;
    if edits.is_empty() {
        return Ok(());
    }
    let mut source = fs::read_to_string(path)?;
    for edit in &edits {
        if source.get(edit.start..edit.end) != Some(edit.old.as_str()) {
            return Err(Error::Unrepresentable(format!(
                "{path}: source changed since the records were extracted \
                 (record {}, attribute line at byte {} no longer matches); \
                 re-extract and re-apply",
                edit.id, edit.start
            )));
        }
    }
    for edit in &edits {
        source.replace_range(edit.start..edit.end, &edit.new);
    }
    atomic_write(&fs::canonicalize(path)?, source.as_bytes())
}

/// Reject any record not extracted from `target` (paths compared after
/// canonicalization) or whose extraction path cannot be canonicalized.
fn require_same_file(records: &[Record], path: &str, target: &Path) -> Result<()> {
    let mut identities: HashMap<&str, std::result::Result<PathBuf, String>> = HashMap::new();
    for record in records {
        let identity = match identities.get(record.file.as_str()) {
            Some(known) => known.clone(),
            None => {
                let fresh = fs::canonicalize(&record.file).map_err(|e| e.to_string());
                identities.insert(record.file.as_str(), fresh.clone());
                fresh
            }
        };
        let record_file = identity.map_err(|why| {
            Error::Unrepresentable(format!(
                "{path}: record {} was extracted from {file:?}, which does \
                 not resolve ({why}); emit matches records to the target by \
                 canonical path, so re-extract from the file being edited",
                record.id,
                file = record.file
            ))
        })?;
        if record_file != target {
            return Err(Error::Unrepresentable(format!(
                "{path}: record {} was extracted from {file:?}, not this \
                 file; re-extract from the file being edited",
                record.id,
                file = record.file
            )));
        }
    }
    Ok(())
}

/// The id items of an attribute line, in order; emit never edits these.
fn id_items(line: &AttrLine) -> Vec<&str> {
    line.items
        .iter()
        .filter_map(|item| match item {
            Item::Id(id) => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

/// Replace the file at `path` with `bytes` atomically: write a sibling temp
/// file, sync it, carry over the target's permissions, and rename it over
/// the target. Every failure before the rename leaves the target untouched;
/// a failure after it is reported along with any leftover temp file.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let name = path.file_name().ok_or_else(|| {
        Error::Unrepresentable(format!("emit target {} has no file name", path.display()))
    })?;
    let name = name.to_string_lossy();
    let pid = std::process::id();
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp = path
        .parent()
        .unwrap_or(Path::new(""))
        .join(format!(".{name}.vaire-{pid}-{seq}.tmp"));
    let failure = match stage(&temp, path, bytes) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    let leftover = match fs::remove_file(&temp) {
        Ok(()) => String::new(),
        Err(cleanup) => format!(
            "; temp file {} remains and must be removed: {cleanup}",
            temp.display()
        ),
    };
    Err(Error::Unrepresentable(format!(
        "atomic write of {} failed: {failure}{leftover}",
        path.display()
    )))
}

/// Write `bytes` to `temp`, sync it, copy `target`'s permissions onto it,
/// and rename it over `target`; all-or-nothing to readers.
fn stage(temp: &Path, target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::set_permissions(temp, fs::metadata(target)?.permissions())?;
    fs::rename(temp, target)
}

/// Convenience for tests and tools: parse a JSON record set.
pub fn records_from_json(json: &str) -> Result<Vec<Record>> {
    serde_json::from_str(json).map_err(Error::Json)
}
