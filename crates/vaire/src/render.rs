//! Rendering for `vaire` command output.
//!
//! Rendering is pure: functions write to any [`std::io::Write`] and take a
//! fixed [`anstream::ColorChoice`], so tests can force colors on or off
//! independent of any TTY detection.

use std::io::{self, Write};

use anstream::ColorChoice;
use anstyle::{AnsiColor, Style};

use vaire::Record;
use vaire::check::Violation;

/// Style for a validation rule code such as `V4`.
const RULE_STYLE: Style = AnsiColor::Red.on_default().bold();
/// Style for file paths.
const FILE_STYLE: Style = AnsiColor::Cyan.on_default();
/// Style for a requirement id.
const ID_STYLE: Style = AnsiColor::Yellow.on_default().bold();
/// Style for headers, column labels, and separators.
const DIM_STYLE: Style = Style::new().dimmed();
/// Style for the `shall` keyword and diff `-` lines.
const SHALL_STYLE: Style = AnsiColor::Red.on_default().bold();
/// Style for the `should` keyword.
const SHOULD_STYLE: Style = AnsiColor::Yellow.on_default();
/// Style for the `may` keyword and diff `@` markers.
const MAY_STYLE: Style = Style::new().dimmed();
/// Style for a status or verification value and diff `+` lines.
const VALUE_STYLE: Style = AnsiColor::Green.on_default();

/// Width at which a statement is truncated in `vaire list`.
const STATEMENT_WIDTH: usize = 60;

/// Render one violation as a single stderr line.
pub fn violation_line(out: &mut impl Write, v: &Violation, choice: ColorChoice) -> io::Result<()> {
    let code = styled(v.rule.code(), RULE_STYLE, choice);
    let file = styled(&v.file, FILE_STYLE, choice);
    writeln!(out, "{code}: {file}: {} [{}]", v.message, v.id)
}

/// Render planned emit edits as a compact diff, ascending by source offset.
pub fn diff(
    out: &mut impl Write,
    path: &str,
    edits: &[vaire::emit::Edit],
    choice: ColorChoice,
) -> io::Result<()> {
    writeln!(out, "{}", styled(&format!("--- {path}"), DIM_STYLE, choice))?;
    // `plan` orders edits by reverse source offset so splices stay valid;
    // the diff reads top-to-bottom.
    for edit in edits.iter().rev() {
        writeln!(
            out,
            "{} {}",
            styled("@", MAY_STYLE, choice),
            styled(&edit.id, ID_STYLE, choice)
        )?;
        writeln!(out, "{} {}", styled("-", SHALL_STYLE, choice), edit.old)?;
        writeln!(out, "{} {}", styled("+", VALUE_STYLE, choice), edit.new)?;
    }
    Ok(())
}

/// Render the `vaire list` table for one file's records.
pub fn table(
    out: &mut impl Write,
    path: &str,
    records: &[&Record],
    choice: ColorChoice,
) -> io::Result<()> {
    writeln!(out, "{}", styled(&format!("--- {path}"), DIM_STYLE, choice))?;
    writeln!(
        out,
        "{}  {}  {}  {}  {}",
        styled("ID", DIM_STYLE, choice),
        styled("KIND", DIM_STYLE, choice),
        styled("MODALITY", DIM_STYLE, choice),
        styled("STATUS", DIM_STYLE, choice),
        styled("VERIFICATION", DIM_STYLE, choice),
    )?;
    for record in records {
        let kind = record.field("kind").unwrap_or_else(|| "-".to_owned());
        let modality = record.field("modality").unwrap_or_else(|| "-".to_owned());
        let status = record.field("status").unwrap_or_else(|| "-".to_owned());
        let verification = record
            .field("verification")
            .unwrap_or_else(|| "-".to_owned());
        writeln!(
            out,
            "{}  {}  {}  {}  {}  {}",
            styled(&record.id, ID_STYLE, choice),
            styled(&kind, DIM_STYLE, choice),
            styled(&modality, modality_style(&modality), choice),
            styled(&status, VALUE_STYLE, choice),
            styled(&verification, VALUE_STYLE, choice),
            truncate(&record.statement()),
        )?;
    }
    Ok(())
}

fn modality_style(modality: &str) -> Style {
    match modality {
        "shall" => SHALL_STYLE,
        "should" => SHOULD_STYLE,
        "may" => MAY_STYLE,
        _ => Style::new(),
    }
}

fn truncate(text: &str) -> String {
    let width = text
        .char_indices()
        .nth(STATEMENT_WIDTH)
        .map_or(text.len(), |(i, _)| i);
    #[expect(
        clippy::string_slice,
        reason = "width comes from char_indices, so it is a char boundary"
    )]
    let head = &text[..width];
    if width < text.len() {
        format!("{}…", head.trim_end())
    } else {
        text.to_owned()
    }
}

fn styled(text: &str, style: Style, choice: ColorChoice) -> String {
    match choice {
        ColorChoice::Never => text.to_owned(),
        _ => format!("{}{text}{}", style.render(), style.render_reset()),
    }
}

#[cfg(test)]
#[path = "tests/render.rs"]
mod tests;
