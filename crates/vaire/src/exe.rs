//! Implementations of `vaire` command-line operations.

use std::io::{self, Write};

use anstream::AutoStream;

use crate::cli::Command;
use crate::render;

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
        Command::Check { files, quiet } => check(&files, quiet),
        Command::List { files } => list(&files),
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

fn check(files: &[String], quiet: bool) -> Result<(), Fail> {
    let violations = vaire::check::check(files)?;
    if violations.is_empty() {
        return Ok(());
    }
    if !quiet {
        let choice = AutoStream::choice(&io::stderr());
        let mut stderr = AutoStream::new(Box::new(io::stderr()) as Box<dyn Write>, choice);
        for violation in &violations {
            render::violation_line(&mut stderr, violation, choice)?;
        }
    }
    Err(Fail::Violations)
}

fn list(files: &[String]) -> Result<(), Fail> {
    let choice = AutoStream::choice(&io::stdout());
    let mut stdout = AutoStream::new(Box::new(io::stdout()) as Box<dyn Write>, choice);
    for file in files {
        let source = std::fs::read_to_string(file).map_err(|e| Fail::Message(e.to_string()))?;
        let records = vaire::extract::extract(file, &source)?;
        render::table(&mut stdout, file, &records, choice)?;
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
