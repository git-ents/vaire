//! Command-line parsing for `vaire`.

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Command-line arguments accepted by `vaire`.
#[derive(Debug, Parser)]
#[command(
    name = "vaire",
    version,
    about = "Requirement records in AsciiDoc: extract, emit, check"
)]
pub(crate) struct Cli {
    /// The operation selected by the caller.
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Operations supported by `vaire`.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Extract requirement records as JSON located by byte spans.
    Extract {
        /// AsciiDoc files to extract.
        #[arg(required = true)]
        files: Vec<String>,
        /// Emit one JSON object per line instead of a pretty document.
        #[arg(long, short = 'c')]
        compact: bool,
    },
    /// Splice edited attribute lines back into a source file, in place.
    Emit {
        /// JSON record set produced by `vaire extract`.
        json: String,
        /// The AsciiDoc file the records were extracted from.
        file: String,
        /// Show the planned rewrites without writing the file.
        #[arg(long, short = 'n', conflicts_with = "diff")]
        dry_run: bool,
        /// Show the planned rewrites as a diff, then write the file.
        #[arg(long)]
        diff: bool,
    },
    /// Validate requirement records against rules V1–V6.
    Check {
        /// AsciiDoc files to validate; cross-file rules see the whole set.
        #[arg(required = true)]
        files: Vec<String>,
        /// Suppress per-violation output; report only the exit code.
        #[arg(long, short = 'q')]
        quiet: bool,
    },
    /// List requirements per file in a table.
    ///
    /// Records appear in file-argument order, then source order (ascending
    /// span start) within each file. All filters are optional and combinable
    /// with AND semantics: a record is listed only when every set filter
    /// matches. Matching is case-sensitive exact everywhere except `--id`,
    /// which is a prefix match.
    List {
        /// AsciiDoc files to list.
        #[arg(required = true)]
        files: Vec<String>,
        /// Record selection and output-format options.
        #[command(flatten)]
        filter: ListFilter,
    },
    /// Show one requirement in human-oriented form.
    Show {
        /// The AsciiDoc file containing the requirement.
        file: String,
        /// The requirement id to show.
        id: String,
    },
    /// Set attributes on one requirement directly in its source file.
    Edit {
        /// The AsciiDoc file containing the requirement.
        file: String,
        /// The requirement id to edit.
        id: String,
        /// Attribute assignments; each key must already be present in the
        /// requirement's attribute lines.
        #[arg(long = "set", value_name = "KEY=VALUE", required = true)]
        sets: Vec<String>,
        /// Show the planned rewrites without writing the file.
        #[arg(long, short = 'n', conflicts_with = "diff")]
        dry_run: bool,
        /// Show the planned rewrites as a diff, then write the file.
        #[arg(long)]
        diff: bool,
    },
}

/// Selection filters and output format for `list`.
///
/// Every filter is optional; set filters combine with AND semantics, and all
/// matching is case-sensitive exact except `--id`, which is a prefix match.
#[derive(Debug, Args)]
pub(crate) struct ListFilter {
    /// List only records whose id starts with this prefix.
    #[arg(long, value_name = "PREFIX")]
    pub(crate) id: Option<String>,

    /// Match records whose merged attributes carry exactly this KEY=VALUE;
    /// repeatable, all must match.
    #[arg(long = "attr", value_name = "KEY=VALUE", value_parser = parse_attr)]
    pub(crate) attrs: Vec<(String, String)>,

    /// Match records with exactly this modality value.
    #[arg(long, value_enum)]
    pub(crate) modality: Option<Modality>,

    /// Match records with exactly this status value.
    #[arg(long, value_name = "STATUS")]
    pub(crate) status: Option<String>,

    /// Match records with exactly this verification value.
    #[arg(long, value_name = "VALUE")]
    pub(crate) verification: Option<String>,

    /// Match records whose merged trace keys (refines, derives-from,
    /// satisfies) reference every listed id; repeatable, comma-separated.
    #[arg(long, value_name = "ID", value_delimiter = ',')]
    pub(crate) traces: Vec<String>,

    /// List only these paths among the given files, matched against the
    /// positional arguments as written; repeatable. Other files are not read.
    #[arg(long, value_name = "PATH")]
    pub(crate) file: Vec<String>,

    /// Output structure; controls layout only, never color.
    #[arg(long, value_enum, default_value = "table")]
    pub(crate) format: Format,
}

/// Output structure of `list`; `--format` selects layout, never color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// Human-readable table, one section per file (the default).
    Table,
    /// Pretty-printed JSON array; absent attributes are `null`.
    Json,
    /// Single-line JSON array, same objects as `json`.
    Compact,
    /// Tab-separated rows with a header; cells flatten tabs and line breaks.
    Tsv,
}

/// Modalities accepted by `--modality`, matched exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Modality {
    Shall,
    Should,
    May,
}

impl Modality {
    /// The attribute value this filter matches.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Shall => "shall",
            Self::Should => "should",
            Self::May => "may",
        }
    }
}

/// Parse one `--attr KEY=VALUE` argument.
fn parse_attr(raw: &str) -> Result<(String, String), String> {
    match raw.split_once('=') {
        Some((key, value)) if !key.is_empty() => Ok((key.to_owned(), value.to_owned())),
        _ => Err("expected KEY=VALUE".to_owned()),
    }
}
