//! Validation rules V1–V6 over a set of extracted files.

use crate::extract::extract;
use crate::{KNOWN_KEYS, MODALITIES, Record, TRACE_KEYS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationRule {
    /// V1 — id uniqueness across the file set.
    DuplicateId,
    /// V2 — trace targets resolve to an id in the file set.
    UnresolvedTrace,
    /// V3 — leaf requirements carry a verification method.
    MissingVerification,
    /// V4 — prose modality agrees with the modality attribute.
    ModalityDisagreement,
    /// V5 — one modality keyword per normative statement.
    CompoundStatement,
    /// V6 — named attributes are in the vocabulary.
    UnknownAttribute,
}

impl ValidationRule {
    pub fn code(self) -> &'static str {
        match self {
            ValidationRule::DuplicateId => "V1",
            ValidationRule::UnresolvedTrace => "V2",
            ValidationRule::MissingVerification => "V3",
            ValidationRule::ModalityDisagreement => "V4",
            ValidationRule::CompoundStatement => "V5",
            ValidationRule::UnknownAttribute => "V6",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule: ValidationRule,
    pub file: String,
    pub id: String,
    pub message: String,
    /// Byte offset in `file` of the offending record; V6 points at the
    /// attribute line carrying the unknown key.
    pub offset: usize,
    /// 1-based line of `offset`, counting `\n` terminators; `None` when the
    /// source bytes were not available to fill it in.
    pub line: Option<usize>,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}: {} [{}]",
            self.rule.code(),
            self.file,
            self.message,
            self.id
        )
    }
}

/// Everything one `check` run saw: how many requirements and what failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    /// Requirement records extracted across the file set.
    pub requirement_count: usize,
    /// All violations, in stable order: file-argument order, then span
    /// start, then rule code.
    pub violations: Vec<Violation>,
}

/// `check`: validate every file; cross-file rules see the whole set.
pub fn check(files: &[String]) -> crate::Result<Vec<Violation>> {
    Ok(check_outcome(files)?.violations)
}

/// `check` plus the requirement count, for report builders.
pub fn check_outcome(files: &[String]) -> crate::Result<CheckOutcome> {
    let mut records: Vec<Record> = Vec::new();
    let mut sources: Vec<(String, String)> = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(file)?;
        records.extend(extract(file, &source)?);
        sources.push((file.clone(), source));
    }
    let mut violations = validate(&records);
    for violation in &mut violations {
        violation.line = sources
            .iter()
            .find(|(name, _)| *name == violation.file)
            .map(|(_, source)| line_of(source, violation.offset));
    }
    Ok(CheckOutcome {
        requirement_count: records.len(),
        violations,
    })
}

/// 1-based line carrying byte offset `at`; `\n`-counted, so LF and CRLF
/// files report the same line numbers.
fn line_of(source: &str, at: usize) -> usize {
    let end = at.min(source.len());
    #[expect(
        clippy::indexing_slicing,
        reason = "end is clamped to the source length, so the slice is in bounds"
    )]
    let before = &source.as_bytes()[..end];
    1 + before.iter().filter(|&&byte| byte == b'\n').count()
}

pub fn validate(records: &[Record]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();

    for (i, r) in records.iter().enumerate() {
        #[expect(
            clippy::indexing_slicing,
            reason = "i is a valid index into ids by loop construction"
        )]
        if ids[..i].contains(&r.id.as_str()) {
            violations.push(violation(
                r,
                ValidationRule::DuplicateId,
                "duplicate requirement id".to_owned(),
            ));
        }

        for key in TRACE_KEYS {
            if let Some(value) = r.field(key) {
                for target in value.split(',').map(str::trim) {
                    if !ids.contains(&target) {
                        violations.push(violation(
                            r,
                            ValidationRule::UnresolvedTrace,
                            format!("{key} target `{target}` not found"),
                        ));
                    }
                }
            }
        }

        let is_parent = records.iter().any(|other| {
            other.id != r.id
                && TRACE_KEYS.iter().any(|k| {
                    other
                        .field(k)
                        .is_some_and(|v| v.split(',').any(|t| t.trim() == r.id))
                })
        });
        if !is_parent && r.field("verification").is_none() {
            violations.push(violation(
                r,
                ValidationRule::MissingVerification,
                "leaf requirement has no verification attribute".to_owned(),
            ));
        }

        let statement = r.statement();
        if r.field("kind").as_deref() == Some("quality") {
            if !r
                .body_raw
                .lines()
                .any(|l| l.trim_start().starts_with("Must::"))
            {
                violations.push(violation(
                    r,
                    ValidationRule::ModalityDisagreement,
                    "quality requirement has no Must entry".to_owned(),
                ));
            }
        } else {
            let modality = r.field("modality").unwrap_or_default();
            let count = MODALITIES
                .iter()
                .chain(std::iter::once(&"must"))
                .map(|kw| count_keyword(&statement, kw))
                .sum::<usize>();
            if count == 0 || count_keyword(&statement, &modality) == 0 {
                violations.push(violation(
                    r,
                    ValidationRule::ModalityDisagreement,
                    format!(
                        "prose does not carry modality `{modality}`: \"{}\"",
                        statement
                    ),
                ));
            }
            if count > 1 {
                violations.push(violation(
                    r,
                    ValidationRule::CompoundStatement,
                    "more than one modality keyword in the statement".to_owned(),
                ));
            }
        }

        for line in &r.attr_lines {
            for item in &line.items {
                if let crate::Item::Kv { name, .. } = item
                    && !KNOWN_KEYS.contains(&name.as_str())
                {
                    violations.push(Violation {
                        rule: ValidationRule::UnknownAttribute,
                        file: r.file.clone(),
                        id: r.id.clone(),
                        message: format!("unknown attribute `{name}`"),
                        offset: line.start,
                        line: None,
                    });
                }
            }
        }
    }
    sort_stable(records, violations)
}

/// A record-level violation, located at the record's span start.
fn violation(record: &Record, rule: ValidationRule, message: String) -> Violation {
    Violation {
        rule,
        file: record.file.clone(),
        id: record.id.clone(),
        message,
        offset: record.span.start,
        line: None,
    }
}

/// Stable violation order: file-argument order, then span start, then rule
/// code; full ties keep emission (source) order.
fn sort_stable(records: &[Record], mut violations: Vec<Violation>) -> Vec<Violation> {
    let mut files: Vec<&str> = Vec::new();
    for record in records {
        if !files.contains(&record.file.as_str()) {
            files.push(record.file.as_str());
        }
    }
    let file_index = |file: &str| {
        files
            .iter()
            .position(|candidate| *candidate == file)
            .unwrap_or_default()
    };
    violations.sort_by_key(|v| (file_index(&v.file), v.offset, v.rule.code()));
    violations
}

fn count_keyword(text: &str, keyword: &str) -> usize {
    let lower = text.to_lowercase();
    let keyword = keyword.to_lowercase();
    let bytes = lower.as_bytes();
    let mut count = 0usize;
    for (i, w) in lower.match_indices(&keyword) {
        #[expect(
            clippy::indexing_slicing,
            reason = "i is a byte offset inside lower, and i > 0 on this branch"
        )]
        let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let after = i + w.len();
        #[expect(
            clippy::indexing_slicing,
            reason = "after is a byte offset inside lower; bounds checked on this branch"
        )]
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            count += 1;
        }
    }
    count
}
