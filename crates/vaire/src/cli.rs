//! Command-line parsing for `vaire`.

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Command-line arguments accepted by `vaire`.
#[derive(Debug, Parser)]
#[command(
    name = "vaire",
    version,
    about = "Requirement records in AsciiDoc: extract, emit, check",
    after_help = EXAMPLES
)]
pub(crate) struct Cli {
    /// When to color human-oriented output: violation lines, the `list`
    /// table, `show`, and diffs. Machine-readable output (`extract`,
    /// `--format json|compact|tsv`) is never colored. `auto` colors a
    /// terminal and honors `NO_COLOR`; `always` and `never` override
    /// detection.
    #[arg(long, value_enum, default_value = "auto", global = true)]
    pub(crate) color: ColorWhen,
    /// The operation selected by the caller.
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Worked examples shown after `vaire --help`; every command matches the
/// option semantics documented on the subcommands.
const EXAMPLES: &str = "\
Examples:
  vaire extract spec.adoc > records.json
      Extract every requirement record to a pretty JSON document.
  vaire extract --compact spec.adoc > records.json
      The same records as one single-line JSON document.
  vaire check spec.adoc derived.adoc
      Validate a file set; cross-file traces resolve across all of them.
  vaire list spec.adoc --modality shall --status draft
      Table of records matching every filter; --format json for machines.
  vaire show spec.adoc SWR-0001
      One requirement: attribute lines, traces, and body.
  vaire emit -n records.json spec.adoc
      Preview the planned rewrites on stderr without writing the file.
  vaire emit --diff records.json spec.adoc
      Show the rewrites as a diff on stderr, then write the file.
  vaire edit -n spec.adoc SWR-0001 --set status=approved
      Preview the attribute edit on stderr; drop -n to apply it in place.";

/// Operations supported by `vaire`.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Extract requirement records as JSON located by byte spans.
    Extract {
        /// AsciiDoc files to extract; a path repeated on the command line
        /// is read once.
        #[arg(required = true)]
        files: Vec<String>,
        /// Emit one compact single-line JSON document instead of a
        /// pretty-printed one.
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
        /// AsciiDoc files to validate; cross-file rules see the whole set,
        /// and a path repeated on the command line is read once.
        #[arg(required = true)]
        files: Vec<String>,
        /// Suppress per-violation output; report only the exit code.
        #[arg(long, short = 'q')]
        quiet: bool,
        /// Print a summary on stdout: file, requirement, and violation
        /// totals, then per-rule counts V1–V6 in fixed order. The summary
        /// always counts every violation, whatever `--rule` displays;
        /// violations stay on stderr, and with `--quiet` only the summary
        /// prints.
        #[arg(long, conflicts_with = "format")]
        summary: bool,
        /// Write one machine-readable report to stdout instead of
        /// per-violation lines; never colored, even on a terminal.
        #[arg(long, value_enum)]
        format: Option<CheckFormat>,
        /// Only display violations of these rules; repeatable. The exit
        /// code and `--summary` still reflect every violation.
        #[arg(long = "rule", value_name = "RULE", value_enum)]
        rules: Vec<RuleCode>,
    },
    /// List requirements per file in a table.
    ///
    /// Records appear in file-argument order, then source order (ascending
    /// span start) within each file. All filters are optional and combinable
    /// with AND semantics: a record is listed only when every set filter
    /// matches. Matching is case-sensitive exact everywhere except `--id`,
    /// which is a prefix match.
    List {
        /// AsciiDoc files to list; a path repeated on the command line is
        /// listed once.
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

/// Report structure of `check`; `--format` selects the report, never color.
/// The default (no `--format`) is one violation line per violation on stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum CheckFormat {
    /// Pretty-printed JSON report object.
    Json,
    /// Single-line JSON report, same object as `json`.
    Compact,
}

/// A validation rule selectable by `--rule`, addressed by its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum RuleCode {
    /// V1 — id uniqueness across the file set.
    #[value(name = "V1")]
    V1,
    /// V2 — trace targets resolve to an id in the file set.
    #[value(name = "V2")]
    V2,
    /// V3 — leaf requirements carry a verification method.
    #[value(name = "V3")]
    V3,
    /// V4 — prose modality agrees with the modality attribute.
    #[value(name = "V4")]
    V4,
    /// V5 — one modality keyword per normative statement.
    #[value(name = "V5")]
    V5,
    /// V6 — named attributes are in the vocabulary.
    #[value(name = "V6")]
    V6,
}

impl RuleCode {
    /// The library rule this code selects.
    pub(crate) fn rule(self) -> vaire::check::ValidationRule {
        match self {
            Self::V1 => vaire::check::ValidationRule::DuplicateId,
            Self::V2 => vaire::check::ValidationRule::UnresolvedTrace,
            Self::V3 => vaire::check::ValidationRule::MissingVerification,
            Self::V4 => vaire::check::ValidationRule::ModalityDisagreement,
            Self::V5 => vaire::check::ValidationRule::CompoundStatement,
            Self::V6 => vaire::check::ValidationRule::UnknownAttribute,
        }
    }
}

/// When to style human-oriented output, selected by the global `--color`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ColorWhen {
    /// Color when the target is a terminal; `NO_COLOR` and `CLICOLOR` apply.
    Auto,
    /// Color regardless of the target, even when piped.
    Always,
    /// Never color, even on a terminal.
    Never,
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
