//! Requirement records in AsciiDoc.
//!
//! `extract` produces JSON records located by byte spans in the source file.
//! `emit` splices edited attribute lines back into the original bytes; every
//! byte outside an edited attribute line passes through untouched.

use std::fmt;

use serde::{Deserialize, Serialize};

pub mod check;
pub mod emit;
pub mod extract;
pub mod show;

pub use check::{ValidationRule, check};
pub use extract::extract;
pub use show::show;

#[derive(Debug)]
pub enum Error {
    Parse(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Unrepresentable(String),
    Validation(String),
    /// A file or record the caller named could not be resolved to exactly one target.
    Missing(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(m) => write!(f, "parse error: {m}"),
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Json(e) => write!(f, "json error: {e}"),
            Error::Unrepresentable(m) => write!(f, "unrepresentable: {m}"),
            Error::Validation(m) => write!(f, "{m}"),
            Error::Missing(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// One parsed item inside an attribute line.
///
/// Style and id come from the positional grammar `[style#id]`; named keys are
/// `name=value`. Anything else acdc or the grammar admits is kept raw and
/// re-emitted verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Item {
    Style(String),
    Id(String),
    Kv { name: String, value: String },
    Raw(String),
}

/// One `[...]` attribute line above a requirement block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttrLine {
    /// Byte offset of the first byte of the line (exclusive of the newline).
    pub start: usize,
    /// Byte offset one past the last byte of the line (exclusive of the newline).
    pub end: usize,
    /// The line as it appears in the source, without the newline.
    pub raw: String,
    pub items: Vec<Item>,
}

/// A requirement record located in one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Path exactly as passed to `extract`. `emit` canonicalizes it to decide
    /// whether the record applies to the target file.
    pub file: String,
    pub id: String,
    /// Byte span of the whole record region: first attribute line through the
    /// closing delimiter line, end exclusive.
    pub span: Span,
    /// Attribute lines in source order; the first carries the style and id.
    pub attr_lines: Vec<AttrLine>,
    /// The block delimiter as written (`--`, `====`, ...).
    pub delim: String,
    /// Raw body bytes between the delimiters, excluding the delimiter lines.
    pub body_raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Record {
    /// The merged attribute view: later lines override earlier ones for the
    /// same key. Style and id are not included.
    pub fn fields(&self) -> Vec<(String, String)> {
        let mut fields: Vec<(String, String)> = Vec::new();
        for line in &self.attr_lines {
            for item in &line.items {
                if let Item::Kv { name, value } = item {
                    if let Some(slot) = fields.iter_mut().find(|(n, _)| n == name) {
                        slot.1 = value.clone();
                    } else {
                        fields.push((name.clone(), value.clone()));
                    }
                }
            }
        }
        fields
    }

    pub fn field(&self, name: &str) -> Option<String> {
        self.fields()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    /// The normative statement: the body's leading paragraph, i.e. the lines
    /// before the first blank line, nested block, or description-list entry.
    pub fn statement(&self) -> String {
        self.body_raw
            .lines()
            .take_while(|l| {
                !l.trim().is_empty() && !l.trim_start().starts_with('.') && !l.contains("::")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Trace-link attribute names, all pointing upward.
pub const TRACE_KEYS: &[&str] = &["refines", "derives-from", "satisfies"];

/// The attribute vocabulary. Unknown named keys are a validation error.
pub const KNOWN_KEYS: &[&str] = &[
    "kind",
    "modality",
    "status",
    "source",
    "verification",
    "volatility",
    "refines",
    "derives-from",
    "satisfies",
    "allocated-to",
    "characteristic",
    "rationale",
];

pub const MODALITIES: &[&str] = &["shall", "should", "may"];
