//! Human-oriented display of one requirement record.
//!
//! Like the binary's other renderers, output goes to any [`std::io::Write`]
//! with a resolved [`anstream::ColorChoice`], so piped runs are plain and
//! tests can force either mode.

use std::io::Write;

use anstream::ColorChoice;
use anstyle::{AnsiColor, Style};

use crate::extract::extract;
use crate::{Error, Item, Record, TRACE_KEYS};

/// Style for section labels and header keys.
const LABEL_STYLE: Style = Style::new().dimmed();
/// Style for the requirement id.
const ID_STYLE: Style = AnsiColor::Yellow.on_default().bold();

/// Write requirement `id` from AsciiDoc `file` to `out` in human-oriented form.
///
/// Reading and extraction happen here so every failure — missing or non-UTF-8
/// file, invalid source, absent or duplicated id — is one error naming `file`,
/// and `out` receives nothing unless exactly one record is rendered.
pub fn show(out: &mut impl Write, file: &str, id: &str, choice: ColorChoice) -> crate::Result<()> {
    let source =
        std::fs::read_to_string(file).map_err(|e| Error::Missing(format!("{file}: {e}")))?;
    let records = extract(file, &source)?;
    let hits: Vec<&Record> = records.iter().filter(|r| r.id == id).collect();
    match hits.as_slice() {
        [] => Err(Error::Missing(format!(
            "{file}: requirement `{id}` not found"
        ))),
        [record] => render(out, file, record, &source, choice),
        dups => Err(Error::Missing(format!(
            "{file}: requirement id `{id}` appears {} times: {}",
            dups.len(),
            dups.iter()
                .map(|r| location(&source, r.span.start))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Render one record: header, raw and parsed attribute lines, traces, body.
fn render(
    out: &mut impl Write,
    file: &str,
    record: &Record,
    source: &str,
    choice: ColorChoice,
) -> crate::Result<()> {
    writeln!(
        out,
        "{}",
        styled(&format!("--- {file}"), LABEL_STYLE, choice)
    )?;
    writeln!(
        out,
        "{} {}",
        styled("id:", LABEL_STYLE, choice),
        styled(&record.id, ID_STYLE, choice)
    )?;
    if let Some(style) = record.attr_lines.first().and_then(|l| {
        l.items.iter().find_map(|i| match i {
            Item::Style(s) => Some(s.as_str()),
            _ => None,
        })
    }) {
        writeln!(out, "{} {style}", styled("style:", LABEL_STYLE, choice))?;
    }
    writeln!(
        out,
        "{} lines {}-{} (bytes {}-{})",
        styled("source:", LABEL_STYLE, choice),
        line_number(source, record.span.start),
        line_number(source, record.span.end),
        record.span.start,
        record.span.end
    )?;
    writeln!(
        out,
        "{} {}",
        styled("delim:", LABEL_STYLE, choice),
        record.delim
    )?;
    writeln!(
        out,
        "{}",
        styled("raw attribute lines:", LABEL_STYLE, choice)
    )?;
    for line in &record.attr_lines {
        writeln!(out, "  {}", line.raw)?;
    }
    let (traces, parsed): (Vec<_>, Vec<_>) = record
        .fields()
        .into_iter()
        .partition(|(name, _)| TRACE_KEYS.contains(&name.as_str()));
    if !parsed.is_empty() {
        writeln!(out, "{}", styled("parsed attributes:", LABEL_STYLE, choice))?;
        for (name, value) in &parsed {
            writeln!(out, "  {} = {value}", styled(name, LABEL_STYLE, choice))?;
        }
    }
    if !traces.is_empty() {
        writeln!(out, "{}", styled("traces:", LABEL_STYLE, choice))?;
        for (name, value) in &traces {
            writeln!(out, "  {} = {value}", styled(name, LABEL_STYLE, choice))?;
        }
    }
    writeln!(out, "{}", styled("body:", LABEL_STYLE, choice))?;
    // The body is echoed verbatim — no recoloring or reserialization, so the
    // bytes after the label are exactly the source bytes between delimiters.
    write!(out, "{}", record.body_raw)?;
    Ok(())
}

/// `line N (byte M)` for a byte offset taken from a record span.
fn location(source: &str, offset: usize) -> String {
    format!("line {} (byte {offset})", line_number(source, offset))
}

/// 1-based number of the source line containing `offset`.
#[expect(
    clippy::indexing_slicing,
    reason = "offsets come from extract's line table over this same source, so they are \
              in bounds and fall on char boundaries"
)]
fn line_number(source: &str, offset: usize) -> usize {
    1 + source.as_bytes()[..offset]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

fn styled(text: &str, style: Style, choice: ColorChoice) -> String {
    match choice {
        ColorChoice::Never => text.to_owned(),
        _ => format!("{}{text}{}", style.render(), style.render_reset()),
    }
}
