//! Command-line parsing for `vaire`.

use clap::{Parser, Subcommand};

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
    List {
        /// AsciiDoc files to list.
        #[arg(required = true)]
        files: Vec<String>,
    },
}
