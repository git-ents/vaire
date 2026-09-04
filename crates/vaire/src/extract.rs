//! Locate requirement blocks via acdc, then re-scan their attribute lines.
//!
//! acdc reports block boundaries as line numbers that are correct in the
//! original source in every case. Its byte offsets are not: the preprocessor
//! normalizes line endings (CRLF → LF) and strips trailing whitespace before
//! the grammar runs, and the post-parse remap corrects offsets only for
//! preprocessor rewrites (includes, dropped comments) — never for that
//! normalization. Extraction therefore reads block boundaries as lines from
//! acdc and resolves every byte span against a line table built from the
//! original source, so LF, CRLF, and mixed-ending documents extract the same
//! way.
//!
//! Attribute lines are the run of `[...]` lines directly above the open
//! delimiter; acdc does not carry them in the block span, and a `.Title`
//! above them is not part of vaire's record model. The block body is never
//! reserialized: `body_raw` is the raw source bytes between the delimiter
//! lines, line terminators included exactly as written.

use acdc_parser::{Block, DelimitedBlock, DelimitedBlockType, Location, parse};
use serde::{Deserialize, Serialize};

use crate::{AttrLine, Error, Item, Record, Span};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Extraction {
    pub file: String,
    pub records: Vec<Record>,
}

/// `extract`: every requirement block in `source`, in source order.
///
/// Per `spec/requirements-syntax.adoc` §2, requirements are `--` open
/// blocks. Unterminated `--` blocks, `====` example blocks carrying the
/// requirement style, and indented requirement blocks are parse errors
/// naming the file and location; none of them ever produces a record.
/// So are attribute-line candidates whose quotes never balance, and
/// requirement anchors that no extracted record covers: both mean acdc
/// would silently drop or corrupt the requirement.
pub fn extract(file: &str, source: &str) -> crate::Result<Vec<Record>> {
    let lines = line_table(source);
    reject_unbalanced_quotes(file, source, &lines)?;
    let doc =
        parse(source, &Default::default()).map_err(|e| Error::Parse(format!("{file}: {e}")))?;
    let doc = doc.document();
    let mut blocks = Vec::new();
    collect_requirement_blocks(&doc.blocks, file, &lines, &mut blocks)?;
    let mut records = Vec::new();
    for block in blocks {
        records.push(build_record(file, source, &lines, block)?);
    }
    reject_indented_requirement(file, source, &lines)?;
    reject_lost_requirements(file, source, &lines, &records)?;
    Ok(records)
}

/// A requirement block's delimiter lines, as line-table indices.
struct DelimiterLines<'a> {
    /// The delimiter as acdc read it (`--`); cross-checked against the source.
    delim: &'a str,
    /// Index of the open delimiter line.
    open: usize,
    /// Index of the close delimiter line; always greater than `open`.
    close: usize,
}

fn collect_requirement_blocks<'a>(
    blocks: &[Block<'a>],
    file: &str,
    lines: &[(usize, usize)],
    out: &mut Vec<DelimiterLines<'a>>,
) -> crate::Result<()> {
    for block in blocks {
        match block {
            Block::Section(s) => collect_requirement_blocks(&s.content, file, lines, out)?,
            Block::DelimitedBlock(d) => collect_delimited(d, file, lines, out)?,
            _ => {}
        }
    }
    Ok(())
}

fn collect_delimited<'a>(
    d: &DelimitedBlock<'a>,
    file: &str,
    lines: &[(usize, usize)],
    out: &mut Vec<DelimiterLines<'a>>,
) -> crate::Result<()> {
    match &d.inner {
        DelimitedBlockType::DelimitedOpen(inner) => {
            let open = delimiter_index(d.open_delimiter_location.as_ref(), file, lines)?;
            let close = match d.close_delimiter_location.as_ref() {
                Some(loc) => delimiter_index(Some(loc), file, lines)?,
                None => {
                    return Err(Error::Parse(format!(
                        "{file}: unterminated `{}` open block at {}: the block must be \
                         closed by a matching `{}` line",
                        d.delimiter,
                        at(lines, open),
                        d.delimiter
                    )));
                }
            };
            if close <= open {
                return Err(Error::Parse(format!(
                    "{file}: `{}` open block at {}: close delimiter precedes the open delimiter",
                    d.delimiter,
                    at(lines, open)
                )));
            }
            collect_requirement_blocks(inner, file, lines, out)?;
            if d.metadata.style == Some("requirement") {
                out.push(DelimiterLines {
                    delim: d.delimiter,
                    open,
                    close,
                });
            }
        }
        DelimitedBlockType::DelimitedExample(inner) => {
            if d.metadata.style == Some("requirement") {
                let open = delimiter_index(d.open_delimiter_location.as_ref(), file, lines)?;
                return Err(Error::Parse(format!(
                    "{file}: requirement style on a `{}` example block at {}: requirement \
                     records are only `--` open blocks",
                    d.delimiter,
                    at(lines, open)
                )));
            }
            collect_requirement_blocks(inner, file, lines, out)?;
        }
        _ => {}
    }
    Ok(())
}

/// Line-table index of a delimiter line from a 1-based acdc position.
///
/// Rejects locations that are missing, originate in an `include::` target
/// (vaire does not follow includes), or fall outside the line table.
fn delimiter_index(
    loc: Option<&Location>,
    file: &str,
    lines: &[(usize, usize)],
) -> crate::Result<usize> {
    let Some(loc) = loc else {
        return Err(Error::Parse(format!(
            "{file}: block delimiter has no source location"
        )));
    };
    if loc.start.file.is_some() {
        return Err(Error::Parse(format!(
            "{file}: block content comes from an include:: chain; vaire does not follow includes"
        )));
    }
    let line = loc.start.line;
    usize::try_from(line)
        .ok()
        .and_then(|line| line.checked_sub(1))
        .filter(|&idx| idx < lines.len())
        .ok_or_else(|| {
            Error::Parse(format!(
                "{file}: block delimiter at line {line} is outside the file"
            ))
        })
}

/// `line N (byte offset M)` for a line-table index, for parse diagnostics.
fn at(lines: &[(usize, usize)], idx: usize) -> String {
    match lines.get(idx) {
        Some(&(start, _)) => format!("line {} (byte offset {start})", idx + 1),
        None => format!("line {}", idx + 1),
    }
}

fn build_record(
    file: &str,
    source: &str,
    lines: &[(usize, usize)],
    block: DelimiterLines<'_>,
) -> crate::Result<Record> {
    // Attribute lines stack directly above the open delimiter; anything
    // higher (a title, a previous block's close) ends the run.
    let mut first_attr = block.open;
    while first_attr > 0
        && lines
            .get(first_attr - 1)
            .is_some_and(|&l| is_attr_line(l, source))
    {
        first_attr -= 1;
    }
    let attr_lines: Vec<AttrLine> = lines
        .iter()
        .skip(first_attr)
        .take(block.open - first_attr)
        .map(|&(s, e)| parse_attr_line(source, s, e))
        .collect();
    let id = attr_lines
        .first()
        .and_then(|l| {
            l.items.iter().find_map(|i| match i {
                Item::Id(id) => Some(id.clone()),
                _ => None,
            })
        })
        .ok_or_else(|| {
            Error::Parse(format!(
                "{file}: requirement block at {} has no id anchor on its first attribute line",
                at(lines, block.open)
            ))
        })?;
    #[expect(
        clippy::indexing_slicing,
        reason = "delimiter_index bounds-checked open and close against this same line \
                  table, and close > open, so open, open + 1, close, and first_attr \
                  (<= open) are all in range"
    )]
    let (delim_line, body_start, close_line, attr_start) = (
        lines[block.open],
        lines[block.open + 1].0,
        lines[block.close],
        lines[first_attr].0,
    );
    #[expect(
        clippy::string_slice,
        reason = "line-table bounds fall on \n or the \r before it, always char boundaries"
    )]
    let delim = source[delim_line.0..delim_line.1].trim();
    if delim != block.delim {
        return Err(Error::Parse(format!(
            "{file}: no `{}` open delimiter on the line at {}",
            block.delim,
            at(lines, block.open)
        )));
    }
    #[expect(
        clippy::string_slice,
        reason = "body bounds are line-table starts, always char boundaries"
    )]
    let body_raw = source[body_start..close_line.0].to_owned();
    Ok(Record {
        file: file.to_owned(),
        id,
        span: Span {
            start: attr_start,
            end: close_line.1,
        },
        attr_lines,
        delim: delim.to_owned(),
        body_raw,
    })
}

/// Reject the indented-requirement shape acdc cannot parse: an attribute line
/// carrying a requirement style or id anchor, followed by attribute lines and
/// then an indented `--`. acdc silently drops such blocks, so without this
/// check the requirement would vanish from the record set.
fn reject_indented_requirement(
    file: &str,
    source: &str,
    lines: &[(usize, usize)],
) -> crate::Result<()> {
    for i in 0..lines.len() {
        let Some(&line) = lines.get(i) else {
            continue;
        };
        if !is_attr_line(line, source) || !marks_requirement(line, source) {
            continue;
        }
        let mut j = i + 1;
        while lines.get(j).is_some_and(|&l| is_attr_line(l, source)) {
            j += 1;
        }
        let Some(&(s, e)) = lines.get(j) else {
            break;
        };
        #[expect(
            clippy::string_slice,
            reason = "line-table bounds fall on \n or the \r before it, always char boundaries"
        )]
        let raw = &source[s..e];
        if raw.starts_with([' ', '\t']) && raw.trim() == "--" {
            return Err(Error::Parse(format!(
                "{file}: indented requirement block at {}: acdc cannot parse indented \
                 open blocks; requirements and their attribute lines must not be indented",
                at(lines, j)
            )));
        }
    }
    Ok(())
}

/// Reject attribute-line candidates whose quotes never balance: under the
/// parser's contract (`\` escapes inside quotes), a `"` still open at end of
/// line leaves the quoted value unterminated, and acdc then attaches the
/// attribute list to the wrong block, silently dropping or corrupting the
/// requirement.
fn reject_unbalanced_quotes(
    file: &str,
    source: &str,
    lines: &[(usize, usize)],
) -> crate::Result<()> {
    for (idx, &span) in lines.iter().enumerate() {
        if !is_attr_line(span, source) || !quote_left_open(span, source) {
            continue;
        }
        return Err(Error::Parse(format!(
            "{file}: unbalanced quotes in attribute line at {}: a `\"` opens a \
             quoted value that never closes; balance and escape quoted values, \
             e.g. rationale=\"a\\\"b, c]\"",
            at(lines, idx)
        )));
    }
    Ok(())
}

/// Whether a `"` is still open at end of the line `(s, e)`, scanning with the
/// parser's quote semantics: inside quotes `\` escapes the next character,
/// outside them it is literal.
fn quote_left_open((s, e): (usize, usize), source: &str) -> bool {
    #[expect(
        clippy::string_slice,
        reason = "line-table bounds fall on \n or the \r before it, always char boundaries"
    )]
    let line = &source[s..e];
    let mut in_quotes = false;
    let mut escaped = false;
    for c in line.chars() {
        if in_quotes && escaped {
            escaped = false;
        } else {
            match c {
                '\\' if in_quotes => escaped = true,
                '"' => in_quotes = !in_quotes,
                _ => {}
            }
        }
    }
    in_quotes
}

/// Every requirement-marking attribute line must land inside some record's
/// span. acdc attaches an attribute list to whatever block follows it, so a
/// stray line between the run and the `--` delimiter silently re-homes the
/// requirement onto a paragraph and the record vanishes; fail loudly on any
/// anchor acdc failed to turn into a record.
fn reject_lost_requirements(
    file: &str,
    source: &str,
    lines: &[(usize, usize)],
    records: &[Record],
) -> crate::Result<()> {
    for (idx, &span) in lines.iter().enumerate() {
        if !is_attr_line(span, source) || !marks_requirement(span, source) {
            continue;
        }
        let covered = records
            .iter()
            .any(|r| r.span.start <= span.0 && span.1 <= r.span.end);
        if !covered {
            return Err(Error::Parse(format!(
                "{file}: requirement anchor at {} produced no requirement record; the \
                 attribute lines must sit directly above a `--` open delimiter with \
                 no stray line between them",
                at(lines, idx)
            )));
        }
    }
    Ok(())
}

/// Whether the `[...]` line marks a requirement: a `requirement` style or an
/// id anchor (`[requirement#X]`, `[requirement]`, `[#X]`).
fn marks_requirement((s, e): (usize, usize), source: &str) -> bool {
    #[expect(
        clippy::string_slice,
        reason = "line-table bounds fall on \n or the \r before it, always char boundaries"
    )]
    let line = &source[s..e];
    parse_items(line.trim()).iter().any(|item| match item {
        Item::Id(_) => true,
        Item::Style(style) => style == "requirement",
        _ => false,
    })
}

/// `(start, end)` byte range per line, excluding line terminators.
fn line_table(source: &str) -> Vec<(usize, usize)> {
    let mut table = Vec::new();
    let mut start = 0;
    for line in source.split('\n') {
        let end = start + line.len();
        table.push((start, if line.ends_with('\r') { end - 1 } else { end }));
        start = end + 1;
    }
    table
}

fn is_attr_line((s, e): (usize, usize), source: &str) -> bool {
    #[expect(
        clippy::string_slice,
        reason = "line-table bounds fall on \n or the \r before it, always char boundaries"
    )]
    let line = source[s..e].trim();
    line.starts_with('[') && line.ends_with(']')
}

/// Parse one `[...]` line into items. Quoted values may contain commas.
pub fn parse_attr_line(source: &str, start: usize, end: usize) -> AttrLine {
    #[expect(
        clippy::string_slice,
        reason = "line-table bounds fall on \n or the \r before it, always char boundaries"
    )]
    let raw = source[start..end].to_owned();
    let items = parse_items(raw.trim());
    AttrLine {
        start,
        end,
        raw,
        items,
    }
}

fn parse_items(line: &str) -> Vec<Item> {
    let inner = line.trim().trim_start_matches('[').trim_end_matches(']');
    split_top_level(inner)
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .flat_map(|s| {
            let s = s.trim();
            if let Some(id) = s.strip_prefix('#') {
                vec![Item::Id(id.to_owned())]
            } else if let Some((name, value)) = s.split_once('=') {
                // A named entry wins over the positional shorthand: `k=v#x`
                // is a value containing `#`, not `Style(k=v)` plus `Id(x)`.
                vec![Item::Kv {
                    name: name.trim().to_owned(),
                    value: unquote(value.trim()),
                }]
            } else if let Some((style, id)) = s.split_once('#') {
                // AsciiDoc shorthand `[style#id]`: style and id share one item.
                let mut items = Vec::new();
                if !style.is_empty() {
                    items.push(Item::Style(style.to_owned()));
                }
                items.push(Item::Id(id.to_owned()));
                items
            } else {
                vec![Item::Style(s.to_owned())]
            }
        })
        .collect()
}

/// Split on commas outside a quoted value. Inside quotes `\` escapes the
/// next character, so `\"` does not close the quote; outside quotes a
/// backslash is an ordinary character.
fn split_top_level(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    for c in s.chars() {
        if in_quotes && escaped {
            escaped = false;
        } else {
            match c {
                '\\' if in_quotes => escaped = true,
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => {
                    parts.push(std::mem::take(&mut current));
                    continue;
                }
                _ => {}
            }
        }
        current.push(c);
    }
    parts.push(current);
    parts
}

/// Decode a parsed value. A fully quoted value (`"…"`) loses its quotes and
/// decodes its escapes: `\\` → `\`, `\"` → `"`, and any other `\x` stays
/// `\x` so hand-written values such as `C:\path` survive. Anything else is
/// literal — bare backslashes are ordinary characters and unbalanced quotes
/// pass through — keeping older unescaped sources parsing unchanged.
fn unquote(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return value.to_owned();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(escaped @ ('"' | '\\')) => out.push(escaped),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            _ => out.push(c),
        }
    }
    out
}

/// Render items back into an attribute line.
///
/// A value is quoted — and escaped inside the quotes (`\` → `\\`, `"` →
/// `\"`) — when it contains `"`, `,`, `]`, or any whitespace: quotes protect
/// `,` and `]` from the top-level splitter, and unquoted values are trimmed
/// by the parser, so padding would not survive. Any other value is plain. A
/// rendered line re-parses to exactly the items it was rendered from, so a
/// value with no single-line representation — one carrying a line break —
/// is rejected instead of being emitted ambiguously.
pub fn render_attr_line(items: &[Item]) -> crate::Result<String> {
    let mut out = String::from("[");
    let mut iter = items.iter().peekable();
    let mut first = true;
    while let Some(item) = iter.next() {
        if !first {
            out.push_str(", ");
        }
        first = false;
        match item {
            Item::Style(s) => {
                if s.contains(['\n', '\r']) {
                    return Err(line_break_error("style"));
                }
                out.push_str(s);
                // The positional shorthand `[style#id]` parses to a Style
                // followed by an Id; render the pair back as one token.
                if let Some(Item::Id(id)) = iter.peek() {
                    if id.contains(['\n', '\r']) {
                        return Err(line_break_error("id anchor"));
                    }
                    out.push('#');
                    out.push_str(id);
                    iter.next();
                }
            }
            Item::Id(id) => {
                if id.contains(['\n', '\r']) {
                    return Err(line_break_error("id anchor"));
                }
                out.push('#');
                out.push_str(id);
            }
            Item::Kv { name, value } => {
                for (what, text) in [("name", name.as_str()), ("value", value.as_str())] {
                    if text.contains(['\n', '\r']) {
                        return Err(line_break_error(&format!("attribute `{name}` {what}")));
                    }
                }
                if name.contains(['=', ',', '"']) {
                    return Err(Error::Unrepresentable(format!(
                        "attribute name `{name}` cannot be rendered: it contains \
                         `=`, `,`, or `\"`"
                    )));
                }
                out.push_str(name);
                out.push('=');
                if value.contains(['"', ',', ']']) || value.chars().any(char::is_whitespace) {
                    out.push('"');
                    for c in value.chars() {
                        match c {
                            '"' => out.push_str("\\\""),
                            '\\' => out.push_str("\\\\"),
                            _ => out.push(c),
                        }
                    }
                    out.push('"');
                } else {
                    out.push_str(value);
                }
            }
            Item::Raw(r) => {
                if r.contains(['\n', '\r']) {
                    return Err(line_break_error("raw item"));
                }
                out.push_str(r);
            }
        }
    }
    out.push(']');
    Ok(out)
}

/// Attribute lines are single-line: a payload with CR or LF has no
/// representation on one.
fn line_break_error(what: &str) -> Error {
    Error::Unrepresentable(format!(
        "{what} contains a line break; attribute lines are single-line and \
         cannot represent CR or LF"
    ))
}

/// Expose the delimiter shape acdc saw, for tests and the spec.
pub fn block_kind(d: &DelimitedBlock<'_>) -> &'static str {
    match &d.inner {
        DelimitedBlockType::DelimitedOpen(_) => "open",
        DelimitedBlockType::DelimitedExample(_) => "example",
        _ => "other",
    }
}
