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

/// `check`: validate every file; cross-file rules see the whole set.
pub fn check(files: &[String]) -> crate::Result<Vec<Violation>> {
    let mut records: Vec<Record> = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(file)?;
        records.extend(extract(file, &source)?);
    }
    Ok(validate(&records))
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
            violations.push(Violation {
                rule: ValidationRule::DuplicateId,
                file: r.file.clone(),
                id: r.id.clone(),
                message: "duplicate requirement id".to_owned(),
            });
        }

        for key in TRACE_KEYS {
            if let Some(value) = r.field(key) {
                for target in value.split(',').map(str::trim) {
                    if !ids.contains(&target) {
                        violations.push(Violation {
                            rule: ValidationRule::UnresolvedTrace,
                            file: r.file.clone(),
                            id: r.id.clone(),
                            message: format!("{key} target `{target}` not found"),
                        });
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
            violations.push(Violation {
                rule: ValidationRule::MissingVerification,
                file: r.file.clone(),
                id: r.id.clone(),
                message: "leaf requirement has no verification attribute".to_owned(),
            });
        }

        let statement = r.statement();
        if r.field("kind").as_deref() == Some("quality") {
            if !r
                .body_raw
                .lines()
                .any(|l| l.trim_start().starts_with("Must::"))
            {
                violations.push(Violation {
                    rule: ValidationRule::ModalityDisagreement,
                    file: r.file.clone(),
                    id: r.id.clone(),
                    message: "quality requirement has no Must entry".to_owned(),
                });
            }
        } else {
            let modality = r.field("modality").unwrap_or_default();
            let count = MODALITIES
                .iter()
                .chain(std::iter::once(&"must"))
                .map(|kw| count_keyword(&statement, kw))
                .sum::<usize>();
            if count == 0 || !keyword_present(&statement, &modality) {
                violations.push(Violation {
                    rule: ValidationRule::ModalityDisagreement,
                    file: r.file.clone(),
                    id: r.id.clone(),
                    message: format!(
                        "prose does not carry modality `{modality}`: \"{}\"",
                        statement
                    ),
                });
            }
            if count > 1 {
                violations.push(Violation {
                    rule: ValidationRule::CompoundStatement,
                    file: r.file.clone(),
                    id: r.id.clone(),
                    message: "more than one modality keyword in the statement".to_owned(),
                });
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
                    });
                }
            }
        }
    }
    violations
}

fn keyword_present(text: &str, keyword: &str) -> bool {
    count_keyword(text, keyword) > 0
}

fn count_keyword(text: &str, keyword: &str) -> usize {
    let lower = text.to_lowercase();
    let keyword = keyword.to_lowercase();
    let bytes = lower.as_bytes();
    let kw = keyword.as_bytes();
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
    let _ = kw;
    count
}
