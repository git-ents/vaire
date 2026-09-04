//! Implementations of `vaire` command-line operations.

use std::io::{self, Write};

use anstream::AutoStream;
use serde::Serialize;

use crate::cli::{CheckFormat, Command, Format, ListFilter, RuleCode};
use crate::render;
use vaire::check::ValidationRule;

/// Execute the selected command-line operation.
pub(crate) fn run(command: Command) -> Result<(), Fail> {
    match command {
        Command::Extract { files, compact } => extract(&files, compact),
        Command::Emit {
            json,
            file,
            dry_run,
            diff,
        } => emit(&json, &file, dry_run, diff),
        Command::Check {
            files,
            quiet,
            summary,
            format,
            rules,
        } => check(&files, quiet, summary, format, &rules),
        Command::List { files, filter } => list(&files, &filter),
        Command::Show { file, id } => show(&file, &id),
        Command::Edit {
            file,
            id,
            sets,
            dry_run,
            diff,
        } => edit(&file, &id, &sets, dry_run, diff),
    }
}

/// Failure modes for command execution.
pub(crate) enum Fail {
    /// A message reported as `vaire: <message>`.
    Message(String),
    /// Validation violations; each has already been printed to stderr.
    Violations,
}

impl From<vaire::Error> for Fail {
    fn from(error: vaire::Error) -> Self {
        Fail::Message(error.to_string())
    }
}

impl From<std::io::Error> for Fail {
    fn from(error: std::io::Error) -> Self {
        Fail::Message(error.to_string())
    }
}

fn extract(files: &[String], compact: bool) -> Result<(), Fail> {
    let mut all = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(file).map_err(|e| Fail::Message(e.to_string()))?;
        all.extend(vaire::extract::extract(file, &source)?);
    }
    let json = if compact {
        serde_json::to_string(&all)
    } else {
        serde_json::to_string_pretty(&all)
    }
    .map_err(|e| Fail::Message(e.to_string()))?;
    println!("{json}");
    Ok(())
}

fn emit(json: &str, file: &str, dry_run: bool, show_diff: bool) -> Result<(), Fail> {
    let records = std::fs::read_to_string(json).map_err(|e| Fail::Message(e.to_string()))?;
    let edits = vaire::emit::plan(&records, file)?;
    if dry_run || show_diff {
        let choice = AutoStream::choice(&io::stderr());
        let mut stderr = AutoStream::new(Box::new(io::stderr()) as Box<dyn Write>, choice);
        render::diff(&mut stderr, file, &edits, choice)?;
        if edits.is_empty() {
            writeln!(stderr, "no changes")?;
        }
    }
    if !dry_run {
        vaire::emit::emit(&records, file)?;
    }
    Ok(())
}

fn check(
    files: &[String],
    quiet: bool,
    summary: bool,
    format: Option<CheckFormat>,
    rules: &[RuleCode],
) -> Result<(), Fail> {
    let outcome = vaire::check::check_outcome(files)?;
    let displayed = displayed(&outcome.violations, rules);
    match format {
        Some(format) => print_report(files.len(), &outcome, &displayed, format)?,
        None => {
            if !quiet {
                let choice = AutoStream::choice(&io::stderr());
                let mut stderr = AutoStream::new(Box::new(io::stderr()) as Box<dyn Write>, choice);
                for violation in &displayed {
                    render::violation_line(&mut stderr, violation, choice)?;
                }
            }
            if summary {
                print_summary(files.len(), outcome.requirement_count, &outcome.violations);
            }
        }
    }
    if outcome.violations.is_empty() {
        Ok(())
    } else {
        Err(Fail::Violations)
    }
}

/// Violations left after `--rule` filtering; no filter displays all.
fn displayed<'a>(
    violations: &'a [vaire::check::Violation],
    rules: &[RuleCode],
) -> Vec<&'a vaire::check::Violation> {
    violations
        .iter()
        .filter(|violation| {
            rules.is_empty() || rules.iter().any(|rule| rule.rule() == violation.rule)
        })
        .collect()
}

/// One violation as exposed by `check`'s machine-readable formats.
#[derive(Serialize)]
struct ViolationRow<'a> {
    rule: &'a str,
    file: &'a str,
    id: &'a str,
    message: &'a str,
    /// 1-based source line; `null` when the line could not be computed.
    line: Option<usize>,
    /// Raw byte offset of the offending record or attribute line.
    offset: usize,
}

/// The `check` machine-readable report.
#[derive(Serialize)]
struct CheckReport<'a> {
    file_count: usize,
    requirement_count: usize,
    /// Every violation found, regardless of `--rule` filtering.
    violation_count: usize,
    /// Violations displayed after `--rule` filtering, in stable order.
    violations: Vec<ViolationRow<'a>>,
}

/// Write the `--format json|compact` report; stdout only, never colored.
fn print_report(
    file_count: usize,
    outcome: &vaire::check::CheckOutcome,
    displayed: &[&vaire::check::Violation],
    format: CheckFormat,
) -> Result<(), Fail> {
    let report = CheckReport {
        file_count,
        requirement_count: outcome.requirement_count,
        violation_count: outcome.violations.len(),
        violations: displayed
            .iter()
            .map(|violation| ViolationRow {
                rule: violation.rule.code(),
                file: &violation.file,
                id: &violation.id,
                message: &violation.message,
                line: violation.line,
                offset: violation.offset,
            })
            .collect(),
    };
    let json = match format {
        CheckFormat::Json => serde_json::to_string_pretty(&report),
        CheckFormat::Compact => serde_json::to_string(&report),
    }
    .map_err(|e| Fail::Message(e.to_string()))?;
    println!("{json}");
    Ok(())
}

/// Write the `--summary` report: totals, then per-rule counts V1–V6 in code
/// order. Always reflects every violation, whatever `--rule` displays.
fn print_summary(
    file_count: usize,
    requirement_count: usize,
    violations: &[vaire::check::Violation],
) {
    println!(
        "checked {}, {}, {}",
        plural("file", file_count),
        plural("requirement", requirement_count),
        plural("violation", violations.len()),
    );
    let by_rule = |rule: ValidationRule| violations.iter().filter(|v| v.rule == rule).count();
    println!(
        "violations by rule: V1={} V2={} V3={} V4={} V5={} V6={}",
        by_rule(ValidationRule::DuplicateId),
        by_rule(ValidationRule::UnresolvedTrace),
        by_rule(ValidationRule::MissingVerification),
        by_rule(ValidationRule::ModalityDisagreement),
        by_rule(ValidationRule::CompoundStatement),
        by_rule(ValidationRule::UnknownAttribute),
    );
}

/// `1 file` / `3 files`.
fn plural(noun: &str, n: usize) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn list(files: &[String], filter: &ListFilter) -> Result<(), Fail> {
    for path in &filter.file {
        if !files.contains(path) {
            return Err(Fail::Message(format!(
                "unknown --file value `{path}`: not among the listed files; expected one of: {}",
                files.join(", ")
            )));
        }
    }
    let selected = read_selected(files, filter)?;
    match filter.format {
        Format::Table => {
            let choice = AutoStream::choice(&io::stdout());
            let mut stdout = AutoStream::new(Box::new(io::stdout()) as Box<dyn Write>, choice);
            for (file, records) in &selected {
                let matched: Vec<&vaire::Record> =
                    records.iter().filter(|r| filter.matches(r)).collect();
                render::table(&mut stdout, file, &matched, choice)?;
            }
        }
        Format::Json => print_json(list_rows(&selected, filter), true)?,
        Format::Compact => print_json(list_rows(&selected, filter), false)?,
        Format::Tsv => write_tsv(&mut io::stdout().lock(), &list_rows(&selected, filter))?,
    }
    Ok(())
}

/// Read and extract each file selected by `--file` (all of them when unset),
/// in argument order, before any output so a failure leaves stdout empty.
fn read_selected<'a>(
    files: &'a [String],
    filter: &ListFilter,
) -> Result<Vec<(&'a str, Vec<vaire::Record>)>, Fail> {
    let mut selected = Vec::new();
    for file in files {
        if !filter.file.is_empty() && !filter.file.iter().any(|f| f == file) {
            continue;
        }
        let source = std::fs::read_to_string(file).map_err(|e| Fail::Message(e.to_string()))?;
        let records = vaire::extract::extract(file, &source)?;
        selected.push((file.as_str(), records));
    }
    Ok(selected)
}

impl ListFilter {
    /// Whether `record` passes: unset filters always match, set ones all
    /// must. `--id` is a prefix; everything else is case-sensitive exact.
    fn matches(&self, record: &vaire::Record) -> bool {
        let fields = record.fields();
        let mut exact = self
            .attrs
            .iter()
            .map(|(key, expected)| (key.as_str(), expected.as_str()))
            .chain(self.status.as_deref().map(|v| ("status", v)))
            .chain(self.verification.as_deref().map(|v| ("verification", v)))
            .chain(self.modality.map(|m| ("modality", m.as_str())));
        exact.all(|(key, expected)| field_of(&fields, key) == Some(expected))
            && self
                .id
                .as_deref()
                .is_none_or(|prefix| record.id.starts_with(prefix))
            && self.traces.iter().all(|id| references_trace(&fields, id))
    }
}

/// Whether any merged trace key carries `id` among its comma-separated
/// values.
fn references_trace(fields: &[(String, String)], id: &str) -> bool {
    fields.iter().any(|(key, value)| {
        vaire::TRACE_KEYS.contains(&key.as_str()) && value.split(',').any(|t| t.trim() == id)
    })
}

/// Merged-attribute lookup: the value `key` carries after later lines
/// override earlier ones.
fn field_of<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(n, _)| n == key)
        .map(|(_, v)| v.as_str())
}

/// One record as exposed by `list`'s machine-readable formats.
///
/// Field order is the serialized order. `modality`, `kind`, `status`, and
/// `verification` hold the merged attribute values and serialize as `null`
/// (JSON) or empty cells (TSV) when the attribute is absent.
#[derive(Serialize)]
struct ListRow<'a> {
    /// Path exactly as passed on the command line.
    file: &'a str,
    id: &'a str,
    /// Byte span of the whole record region in the source file.
    span: vaire::Span,
    modality: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    verification: Option<String>,
    /// The body's leading paragraph, as in the human table.
    statement: String,
}

/// Rows for every matching record, in file-argument then source order.
fn list_rows<'a>(
    selected: &'a [(&'a str, Vec<vaire::Record>)],
    filter: &ListFilter,
) -> Vec<ListRow<'a>> {
    let mut rows = Vec::new();
    for (file, records) in selected {
        for record in records.iter().filter(|r| filter.matches(r)) {
            let fields = record.fields();
            let field = |key: &str| field_of(&fields, key).map(str::to_owned);
            rows.push(ListRow {
                file,
                id: &record.id,
                span: record.span,
                modality: field("modality"),
                kind: field("kind"),
                status: field("status"),
                verification: field("verification"),
                statement: record.statement(),
            });
        }
    }
    rows
}

/// Serialize `list` rows as JSON on stdout, pretty or single-line.
fn print_json(rows: Vec<ListRow<'_>>, pretty: bool) -> Result<(), Fail> {
    let json = if pretty {
        serde_json::to_string_pretty(&rows)
    } else {
        serde_json::to_string(&rows)
    }
    .map_err(|e| Fail::Message(e.to_string()))?;
    println!("{json}");
    Ok(())
}

/// Column order of `--format tsv`: the JSON fields with `span` flattened.
const TSV_COLUMNS: &[&str] = &[
    "file",
    "id",
    "span_start",
    "span_end",
    "modality",
    "kind",
    "status",
    "verification",
    "statement",
];

/// Write `list` rows as tab-separated lines with a header row.
fn write_tsv(out: &mut impl Write, rows: &[ListRow<'_>]) -> io::Result<()> {
    writeln!(out, "{}", TSV_COLUMNS.join("\t"))?;
    for row in rows {
        let cells = [
            tsv_cell(row.file),
            tsv_cell(row.id),
            tsv_cell(&row.span.start.to_string()),
            tsv_cell(&row.span.end.to_string()),
            tsv_cell(row.modality.as_deref().unwrap_or_default()),
            tsv_cell(row.kind.as_deref().unwrap_or_default()),
            tsv_cell(row.status.as_deref().unwrap_or_default()),
            tsv_cell(row.verification.as_deref().unwrap_or_default()),
            tsv_cell(&row.statement),
        ];
        writeln!(out, "{}", cells.join("\t"))?;
    }
    Ok(())
}

/// Flatten tabs and line terminators so a cell stays on one row.
fn tsv_cell(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

fn show(file: &str, id: &str) -> Result<(), Fail> {
    let choice = AutoStream::choice(&io::stdout());
    let mut stdout = AutoStream::new(Box::new(io::stdout()) as Box<dyn Write>, choice);
    vaire::show::show(&mut stdout, file, id, choice)?;
    Ok(())
}

fn edit(
    file: &str,
    id: &str,
    raw_sets: &[String],
    dry_run: bool,
    show_diff: bool,
) -> Result<(), Fail> {
    let sets = raw_sets
        .iter()
        .map(|raw| vaire::edit::Set::parse(raw))
        .collect::<vaire::Result<Vec<_>>>()?;
    let edits = vaire::edit::plan(file, id, &sets)?;
    let choice = AutoStream::choice(&io::stderr());
    let mut stderr = AutoStream::new(Box::new(io::stderr()) as Box<dyn Write>, choice);
    if dry_run || show_diff {
        render::diff(&mut stderr, file, &edits, choice)?;
    }
    if edits.is_empty() {
        writeln!(stderr, "no changes")?;
        return Ok(());
    }
    if !dry_run {
        vaire::edit::edit(file, id, &sets)?;
        for set in &sets {
            println!("edited {file}: {id}: {}", set.key);
        }
    }
    Ok(())
}

/// Convert a [`Fail`] into a process exit code, printing as appropriate.
pub(crate) fn exit_code(fail: Fail) -> std::process::ExitCode {
    match fail {
        Fail::Message(message) => {
            eprintln!("vaire: {message}");
            std::process::ExitCode::FAILURE
        }
        Fail::Violations => std::process::ExitCode::FAILURE,
    }
}
